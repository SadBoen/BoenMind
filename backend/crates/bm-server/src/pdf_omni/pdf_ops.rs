//! PDF 本地操作（lopdf 纯 Rust，无系统依赖）：页数检测、按页切分、单页提取、
//! 2×2 网格拼页、A4 画布图片拼接。
//!
//! 原理吸收自 Hermes pdf-omni 插件（pypdf 实现，2026-08 实测结论）：
//! - 2×2 等比缩放拼页（97% 保留率）优于竖排单向压缩（94%）；本地拼页是矢量操作零损耗
//! - 表格/小图按原尺寸拼 A4 画布（100% 细节、mermaid 可触发），一张 A4 ≈ 1 页计费
//! - 拼页实现采用 hipdf 的 Form XObject 方案：源页整体包成 XObject（Resources
//!   自包含，字体/图片引用仍指向原文档对象，零复制），目标页只需一条 cm 变换
//!
//! 全部为同文档操作（load → 增删对象 → save），避免跨文档对象重映射。

use std::path::{Path, PathBuf};

use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use thiserror::Error;

/// A4 画布 @150dpi（与 Hermes 版一致：1240×1754px）
const A4_W: u32 = 1240;
const A4_H: u32 = 1754;
/// A4 页边距与图间距（px @150dpi）
const A4_MARGIN: i64 = 22;
const A4_GAP: i64 = 15;

#[derive(Debug, Error)]
pub enum PdfOpsError {
    #[error("lopdf: {0}")]
    Lopdf(#[from] lopdf::Error),
    #[error("PDF 读取失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("图片解码失败({name}): {source}")]
    ImageDecode {
        name: String,
        source: image::ImageError,
    },
    #[error("PDF 无页面: {0}")]
    EmptyPdf(String),
    #[error("PDF 结构异常: {0}")]
    Corrupt(String),
    #[error("请求的页面越界: 请求 {requested}，共 {total} 页")]
    PageOutOfRange { requested: usize, total: usize },
}

pub type PdfOpsResult<T> = Result<T, PdfOpsError>;

/// 页数检测；解析失败或空文档返回错误（由调用方决定是否忽略）。
pub fn page_count(path: &Path) -> PdfOpsResult<usize> {
    let doc = Document::load(path)?;
    let n = doc.get_pages().len();
    if n == 0 {
        return Err(PdfOpsError::EmptyPdf(path.display().to_string()));
    }
    Ok(n)
}

/// 构造只含 `keep`（0-based 页序）的子文档并保存。
///
/// 单文档删页方案：load 原文件 → 删除非保留页对象 → 重建单层 Pages 树 →
/// save。保留页的 Contents/Resources 引用对象全部原样保留（孤儿对象无害）。
fn subset_pages(path: &Path, keep: &[usize], out: &Path) -> PdfOpsResult<PathBuf> {
    let mut doc = Document::load(path)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    if keep.iter().any(|&i| i >= pages.len()) {
        return Err(PdfOpsError::PageOutOfRange {
            requested: *keep.iter().max().unwrap(),
            total: pages.len(),
        });
    }
    let keep_set: std::collections::HashSet<usize> = keep.iter().copied().collect();
    let retained: Vec<ObjectId> = pages
        .iter()
        .enumerate()
        .filter(|(i, _)| keep_set.contains(i))
        .map(|(_, id)| *id)
        .collect();
    if retained.is_empty() {
        return Err(PdfOpsError::EmptyPdf(path.display().to_string()));
    }

    // 删除非保留页对象（保留页对象、资源对象不动）
    for (i, id) in pages.iter().enumerate() {
        if !keep_set.contains(&i) {
            doc.delete_object(*id);
        }
    }
    rebuild_pages_tree(&mut doc, &retained)?;
    doc.save(out)?;
    Ok(out.to_path_buf())
}

/// 重建单层 Pages 树：替换 catalog 指向的 Pages 节点，Kids = 指定页，更新各页 Parent。
fn rebuild_pages_tree(doc: &mut Document, page_ids: &[ObjectId]) -> PdfOpsResult<()> {
    let catalog = doc.catalog()?.get(b"Pages").map_err(|_| {
        PdfOpsError::Corrupt("catalog 缺少 Pages".into())
    })?;
    let _old_pages_id = catalog.as_reference()?;

    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
        "Count" => page_ids.len() as i64,
    });
    for id in page_ids {
        let page = doc.get_dictionary_mut(*id)?;
        page.set("Parent", pages_id);
    }
    doc.catalog_mut()?.set("Pages", pages_id);
    // 旧 Pages 节点（及其可能的中间层）已无引用，保留为孤儿对象即可
    Ok(())
}

/// 按页切分：页数超过 `chunk_pages` 时切成多块，返回块路径列表；未超限返回 [原路径]。
pub fn split_by_pages(path: &Path, chunk_pages: usize, out_dir: &Path) -> PdfOpsResult<Vec<PathBuf>> {
    let total = page_count(path)?;
    if total <= chunk_pages {
        return Ok(vec![path.to_path_buf()]);
    }
    std::fs::create_dir_all(out_dir)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "pdf".to_string());
    let mut parts = Vec::new();
    for start in (0..total).step_by(chunk_pages) {
        let end = (start + chunk_pages).min(total);
        let keep: Vec<usize> = (start..end).collect();
        let out = out_dir.join(format!("{stem}_p{}-{}.pdf", start + 1, end));
        subset_pages(path, &keep, &out)?;
        parts.push(out);
    }
    Ok(parts)
}

/// 提取指定页（0-based）为独立 PDF，返回子文件路径。级联增强桶 2（大图页单独提交）用。
pub fn extract_pages(path: &Path, page_indices: &[usize], out_dir: &Path) -> PdfOpsResult<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "pdf".to_string());
    let out = out_dir.join(format!("{stem}_cascade.pdf"));
    let mut indices: Vec<usize> = page_indices.to_vec();
    indices.sort_unstable();
    indices.dedup();
    subset_pages(path, &indices, &out)?;
    Ok(out)
}

/// 2×2 网格拼页：每 4 页一组拼成一张等比缩放（0.5×0.5）的大页，超出续排新页。
///
/// 排布（阅读序，PDF 坐标原点在左下）：左上=第1页，左下=第2页，右上=第3页，右下=第4页。
/// 返回 (拼页 PDF 路径, 页组分桶 [[idx...], ...])。
///
/// 实现（hipdf XObject 方案）：源页整体包成 Form XObject（BBox=页 MediaBox，
/// Resources=页 Resources 原样引用，零复制），新页 content 流写 cm 变换 + Do。
pub fn grid_merge_2x2(
    path: &Path,
    page_indices: &[usize],
    out_dir: &Path,
) -> PdfOpsResult<(PathBuf, Vec<Vec<usize>>)> {
    let mut doc = Document::load(path)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let mut indices: Vec<usize> = page_indices.to_vec();
    indices.sort_unstable();
    indices.dedup();
    if indices.iter().any(|&i| i >= pages.len()) {
        return Err(PdfOpsError::PageOutOfRange {
            requested: *indices.iter().max().unwrap(),
            total: pages.len(),
        });
    }

    // 画布尺寸取第一页 MediaBox（Hermes 版同逻辑；混合尺寸文档取基准页）
    let (w, h) = page_mediabox_size(&doc, pages[indices[0]])?;
    let quadrants: [(f64, f64); 4] = [(0.0, h as f64), (0.0, 0.0), (w as f64, h as f64), (w as f64, 0.0)];

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut new_page_ids: Vec<ObjectId> = Vec::new();
    for chunk in indices.chunks(4) {
        groups.push(chunk.to_vec());
        // 组内每页 → Form XObject
        let mut xobject_refs: Vec<(String, ObjectId)> = Vec::new();
        for (j, &idx) in chunk.iter().enumerate() {
            let page_id = pages[idx];
            let xobj_id = wrap_page_as_xobject(&mut doc, page_id)?;
            xobject_refs.push((format!("Fm{}", j + 1), xobj_id));
        }
        // 新页 content：q 0.5 0 0 0.5 tx ty cm /FmN Do Q
        let mut ops: Vec<u8> = Vec::new();
        for (j, &idx) in chunk.iter().enumerate() {
            let (tx, ty) = quadrants[j];
            let name = format!("/Fm{}", j + 1);
            ops.extend_from_slice(
                format!("q 0.5 0 0 0.5 {tx} {ty} cm {name} Do Q\n").as_bytes(),
            );
            let _ = idx;
        }
        let content_id = doc.add_object(Stream::new(dictionary! {}, ops));
        let mut xo = lopdf::Dictionary::new();
        for (n, id) in &xobject_refs {
            xo.set(n.clone(), Object::Reference(*id));
        }
        let resources = dictionary! {
            "XObject" => xo,
        };
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(w * 2.0),
                Object::Real(h * 2.0),
            ],
            "Contents" => content_id,
            "Resources" => resources,
        });
        new_page_ids.push(page_id);
    }

    // 删除被拼接的源页对象（其资源对象保留；XObject 已持有 Resources 引用）
    let consumed: std::collections::HashSet<usize> = indices.iter().copied().collect();
    for (i, id) in pages.iter().enumerate() {
        if consumed.contains(&i) {
            doc.delete_object(*id);
        }
    }
    rebuild_pages_tree(&mut doc, &new_page_ids)?;

    std::fs::create_dir_all(out_dir)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "pdf".to_string());
    let out = out_dir.join(format!("{stem}_grid.pdf"));
    doc.save(&out)?;
    Ok((out, groups))
}

/// 取页 MediaBox 宽高（pt）；缺省回退 A4 尺寸。
fn page_mediabox_size(doc: &Document, page_id: ObjectId) -> PdfOpsResult<(f32, f32)> {
    let page = doc.get_dictionary(page_id)?;
    let w = page
        .get(b"MediaBox")
        .ok()
        .and_then(|o| o.as_array().ok())
        .and_then(|a| a.get(2))
        .and_then(|o| o.as_float().ok())
        .unwrap_or(595.0);
    let h = page
        .get(b"MediaBox")
        .ok()
        .and_then(|o| o.as_array().ok())
        .and_then(|a| a.get(3))
        .and_then(|o| o.as_float().ok())
        .unwrap_or(842.0);
    Ok((w, h))
}

/// 把整页包装成 Form XObject：
/// - XObject stream = 页 Contents 流内容（可能多流，顺序拼接）
/// - XObject Resources = 页 Resources 原样（引用对象仍在原文档，零复制）
/// - BBox = 页 MediaBox
fn wrap_page_as_xobject(doc: &mut Document, page_id: ObjectId) -> PdfOpsResult<ObjectId> {
    let (bbox, resources, content) = {
        let page = doc.get_dictionary(page_id)?.clone();
        let media = page
            .get(b"MediaBox")
            .cloned()
            .unwrap_or_else(|_| Object::Array(vec![Object::Integer(0), Object::Integer(0), Object::Integer(595), Object::Integer(842)]));
        let resources = page
            .get(b"Resources")
            .cloned()
            .unwrap_or_else(|_| Object::Dictionary(lopdf::Dictionary::new()));
        let content = match page.get(b"Contents").ok() {
            Some(Object::Reference(id)) => doc.get_object(*id)?.as_stream()?.content.clone(),
            Some(Object::Array(refs)) => {
                let mut buf = Vec::new();
                for r in refs {
                    if let Ok(id) = r.as_reference()
                        && let Ok(stream) = doc.get_object(id)?.as_stream()
                    {
                        buf.extend_from_slice(&stream.content);
                        buf.push(b'\n');
                    }
                }
                buf
            }
            Some(_) | None => Vec::new(),
        };
        (media, resources, content)
    };
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => bbox,
        "Resources" => resources,
    };
    Ok(doc.add_object(Stream::new(dict, content)))
}

/// 把多张图片按原尺寸贪心排布到 A4 画布（@150dpi），能放几个放几个，超 A4 续排。
///
/// - 图片按原尺寸摆放（不缩放）——小表格/小图拼接的理想方式，细节 100% 保留
/// - 单图超 A4 时等比缩入（兜底，调用方应已过滤大图）
/// - 返回拼接图文件路径列表；每张 A4 图 = LlamaParse 1 页计费
pub fn pack_images_a4(img_paths: &[PathBuf], out_dir: &Path) -> PdfOpsResult<Vec<PathBuf>> {
    let mut images: Vec<(image::DynamicImage, i64, i64)> = Vec::new();
    for p in img_paths {
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let img = image::ImageReader::open(p)
            .map_err(|e| PdfOpsError::ImageDecode { name: name.clone(), source: image::ImageError::IoError(e) })?
            .with_guessed_format()
            .map_err(|e| PdfOpsError::ImageDecode { name: name.clone(), source: image::ImageError::IoError(e) })?
            .decode()
            .map_err(|e| PdfOpsError::ImageDecode { name: name.clone(), source: e })?;
        let mut w = img.width() as i64;
        let mut h = img.height() as i64;
        // 单图超 A4 内容区则等比缩入（兜底）
        let max_w = A4_W as i64 - 2 * A4_MARGIN;
        let max_h = A4_H as i64 - 2 * A4_MARGIN;
        if w > max_w || h > max_h {
            let scale = ((max_w as f64 / w as f64).min(max_h as f64 / h as f64)).min(1.0);
            w = (w as f64 * scale) as i64;
            h = (h as f64 * scale) as i64;
        }
        images.push((img, w, h));
    }
    // 按高度降序（Hermes 版同逻辑，先放大图）
    images.sort_by_key(|img| std::cmp::Reverse(img.2));

    // 贪心排布：换行/换页
    type PlacedImage<'a> = (&'a image::DynamicImage, i64, i64, i64, i64);
    let mut pages: Vec<Vec<PlacedImage>> = Vec::new();
    let mut current: Vec<PlacedImage> = Vec::new(); // (img, w, h, x, y)
    let mut x = A4_MARGIN;
    let mut y = A4_MARGIN;
    let mut row_h: i64 = 0;
    for (img, w, h) in &images {
        if current.is_empty() {
            x = A4_MARGIN;
            y = A4_MARGIN;
            row_h = 0;
        } else if x + w + A4_MARGIN > A4_W as i64 {
            x = A4_MARGIN;
            y += row_h + A4_GAP;
            row_h = 0;
        }
        if y + h + A4_MARGIN > A4_H as i64 {
            pages.push(std::mem::take(&mut current));
            x = A4_MARGIN;
            y = A4_MARGIN;
            row_h = 0;
        }
        current.push((img, *w, *h, x, y));
        x += w + A4_GAP;
        row_h = row_h.max(*h);
    }
    if !current.is_empty() {
        pages.push(current);
    }

    std::fs::create_dir_all(out_dir)?;
    let mut out_paths = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let mut canvas = image::RgbImage::new(A4_W, A4_H);
        for (img, w, h, px, py) in page {
            let resized = img.resize(*w as u32, *h as u32, image::imageops::FilterType::Lanczos3);
            let rgb = resized.to_rgb8();
            for (dx, dy, pixel) in rgb.enumerate_pixels() {
                let (cx, cy) = (*px as u32 + dx, *py as u32 + dy);
                if cx < A4_W && cy < A4_H {
                    canvas.put_pixel(cx, cy, *pixel);
                }
            }
        }
        let out = out_dir.join(format!("packed_a4_{}.png", i + 1));
        canvas.save(&out).map_err(|e| PdfOpsError::ImageDecode {
            name: out.display().to_string(),
            source: e,
        })?;
        out_paths.push(out);
    }
    Ok(out_paths)
}

/// 清理临时目录（级联增强/切分产物）。
pub fn cleanup_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 测试辅助：生成带 N 页的最小 PDF（每页一行文本）。
#[cfg(test)]
pub fn make_test_pdf(n_pages: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = Document::new();
    let pages_id = doc.add_object(dictionary! {});
    let mut page_ids = Vec::new();
    for i in 0..n_pages {
        let content = format!("BT /F1 24 Tf 72 720 Td (page {}) Tj ET", i + 1);
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
        });
        page_ids.push(page_id);
    }
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let pages_obj = dictionary! {
        "Type" => "Pages",
        "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
        "Count" => page_ids.len() as i64,
        "Resources" => resources_id,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages_obj));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.compress();
    let path = dir.path().join("test.pdf");
    doc.save(&path).unwrap();
    (dir, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_count_roundtrip() {
        let (_dir, path) = make_test_pdf(5);
        assert_eq!(page_count(&path).unwrap(), 5);
    }

    #[test]
    fn split_under_limit_returns_original() {
        let (_dir, path) = make_test_pdf(3);
        let out = tempfile::tempdir().unwrap();
        let parts = split_by_pages(&path, 190, out.path()).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], path);
    }

    #[test]
    fn split_over_limit_chunks() {
        let (_dir, path) = make_test_pdf(6);
        let out = tempfile::tempdir().unwrap();
        let parts = split_by_pages(&path, 4, out.path()).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(page_count(&parts[0]).unwrap(), 4);
        assert_eq!(page_count(&parts[1]).unwrap(), 2);
    }

    #[test]
    fn extract_pages_subset() {
        let (_dir, path) = make_test_pdf(6);
        let out = tempfile::tempdir().unwrap();
        let sub = extract_pages(&path, &[1, 3], out.path()).unwrap();
        assert_eq!(page_count(&sub).unwrap(), 2);
    }

    #[test]
    fn extract_out_of_range_rejected() {
        let (_dir, path) = make_test_pdf(3);
        let out = tempfile::tempdir().unwrap();
        assert!(extract_pages(&path, &[5], out.path()).is_err());
    }

    #[test]
    fn grid_merge_2x2_groups_and_pages() {
        let (_dir, path) = make_test_pdf(7);
        let out = tempfile::tempdir().unwrap();
        let (grid, groups) = grid_merge_2x2(&path, &[0, 1, 2, 3, 4, 5], out.path()).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec![0, 1, 2, 3]);
        assert_eq!(groups[1], vec![4, 5]);
        assert_eq!(page_count(&grid).unwrap(), 2);
    }

    #[test]
    fn grid_merge_single_group() {
        let (_dir, path) = make_test_pdf(4);
        let out = tempfile::tempdir().unwrap();
        let (grid, groups) = grid_merge_2x2(&path, &[0, 1, 2, 3], out.path()).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(page_count(&grid).unwrap(), 1);
        // 拼页 MediaBox 应为 2 倍宽高
        let doc = Document::load(&grid).unwrap();
        let (w, h) = page_mediabox_size(&doc, *doc.get_pages().values().next().unwrap()).unwrap();
        assert!((w - 1224.0).abs() < 0.01 && (h - 1584.0).abs() < 0.01);
    }

    #[test]
    fn pack_images_a4_basic() {
        // 造两张小图
        let dir = tempfile::tempdir().unwrap();
        let img1 = dir.path().join("t1.png");
        let img2 = dir.path().join("t2.png");
        image::RgbImage::from_pixel(200, 100, image::Rgb([10, 20, 30]))
            .save(&img1)
            .unwrap();
        image::RgbImage::from_pixel(150, 80, image::Rgb([200, 100, 50]))
            .save(&img2)
            .unwrap();
        let out = tempfile::tempdir().unwrap();
        let packed = pack_images_a4(&[img1, img2], out.path()).unwrap();
        assert_eq!(packed.len(), 1); // 两张小图应进同一张 A4
        let img = image::ImageReader::open(&packed[0]).unwrap().decode().unwrap();
        assert_eq!((img.width(), img.height()), (A4_W, A4_H));
    }

    #[test]
    fn pack_images_a4_overflow_pages() {
        // 4 张大图 → 多张 A4
        let dir = tempfile::tempdir().unwrap();
        let mut imgs = Vec::new();
        for i in 0..4 {
            let p = dir.path().join(format!("big{i}.png"));
            image::RgbImage::from_pixel(900, 1300, image::Rgb([i as u8, 0, 0]))
                .save(&p)
                .unwrap();
            imgs.push(p);
        }
        let out = tempfile::tempdir().unwrap();
        let packed = pack_images_a4(&imgs, out.path()).unwrap();
        assert!(packed.len() >= 2);
    }
}
