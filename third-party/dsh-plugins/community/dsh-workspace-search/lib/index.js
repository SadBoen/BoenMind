/**
 * dsh-workspace-search — out-of-tree bundle for DeepSeek Harness (dsh web).
 *
 * Host half: a VS Code-style workspace keyword search backend carried on the
 * Connection's generic RPC channel registry:
 *
 *   channel /workspace-search
 *     endpoint `search`  {root, query, caseSensitive?, maxFiles?, maxMatches?}
 *
 * Walks the tree with bounded breadth (skip .git/node_modules/hidden by
 * default), matches the query against file NAMES and text-file CONTENT lines,
 * and returns per-file match groups with line numbers. All caps are honored
 * and reported so the client can render "truncated" states honestly.
 */
import z from '@deepseek-ai/schemastery'
import { readdir, readFile, stat } from 'node:fs/promises'
import { isAbsolute, join, basename, relative } from 'node:path'

export const name = 'workspace-search'

/** The host Connection whose RPC registry carries this plugin's channel. */
export const inject = ['connection']

export const Config = z.object({
  /** Hard cap on files scanned per search. Defaults to 5000. */
  maxFiles: z.number().step(1).min(1).default(5000),
  /** Hard cap on total content matches per search. Defaults to 300. */
  maxMatches: z.number().step(1).min(1).default(300),
  /** Hard cap on one matched line's reported length. Defaults to 300. */
  maxLineLength: z.number().step(1).min(20).default(300),
  /** Files above this size are skipped for content scan. Defaults to 1 MiB. */
  maxFileBytes: z.number().step(1).min(1024).default(1048576),
})

/** Directories never entered during the walk. */
const SKIP_DIRS = new Set(['.git', 'node_modules', '.hg', '.svn', 'dist', 'build', '.next', 'target'])

function internalError(message) {
  return { ok: false, error: { code: 'internal', message, details: {} } }
}

function ok(value) {
  return { ok: true, value }
}

function looksBinary(buf) {
  const head = buf.subarray(0, Math.min(buf.length, 8192))
  for (const byte of head) if (byte === 0) return true
  return false
}

async function* walkFiles(dir, root, matcher, seen) {
  let entries
  try {
    entries = await readdir(dir, { withFileTypes: true })
  } catch {
    return
  }
  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue
    const full = join(dir, entry.name)
    if (entry.isSymbolicLink()) continue
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue
      const rel = relPath(root, full)
      if (matcher.excludeDir(rel)) continue
      if (seen.has(full)) continue
      seen.add(full)
      yield* walkFiles(full, root, matcher, seen)
      continue
    }
    if (entry.isFile()) yield full
  }
}

/**
 * VS Code-flavored glob → RegExp. Supports double-star (any depth), `*`
 * (within a segment), `?` (one char), and `{a,b}` alternation. A pattern
 * without a slash matches the basename anywhere; a pattern with a slash
 * matches the root-relative path (a leading double-star-slash is allowed,
 * a leading slash is stripped).
 */
function globToRegExp(pattern) {
  let pat = pattern.replace(/\\/g, '/')
  const leading = pat.startsWith('**/') ? '**/' : ''
  if (leading !== '') pat = pat.slice(3)
  if (pat.startsWith('/')) pat = pat.slice(1)
  const hasSlash = pat.includes('/')
  let re = ''
  for (let i = 0; i < pat.length; i++) {
    const c = pat[i]
    if (c === '*') {
      if (pat[i + 1] === '*') {
        re += '.*'
        i += 1
        if (pat[i + 1] === '/') i += 1
      } else {
        re += '[^/]*'
      }
    } else if (c === '?') {
      re += '[^/]'
    } else if (c === '{') {
      const close = pat.indexOf('}', i)
      if (close === -1) {
        re += '\\{'
      } else {
        const alts = pat.slice(i + 1, close).split(',').map(escapeRegex)
        re += `(?:${alts.join('|')})`
        i = close
      }
    } else {
      re += escapeRegex(c)
    }
  }
  if (hasSlash) {
    return new RegExp(`^(?:${re})${leading === '**/' ? '' : ''}`)
  }
  return new RegExp(`^[^/]*${re}$`)
}

function escapeRegex(chunk) {
  return chunk.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function relPath(root, full) {
  const rel = relative(root, full).replace(/\\/g, '/')
  return rel
}

function compileGlobs(list, dirMode) {
  const patterns = String(list ?? '')
    .split(',')
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
  const regexes = []
  for (const p of patterns) {
    try {
      const re = globToRegExp(p)
      regexes.push({ pattern: p, re, dirMode })
    } catch {
      // Invalid glob: ignored (VS Code behavior for syntactically odd patterns).
    }
  }
  return regexes
}

async function searchEndpoint(payload, config) {
  const root = typeof payload?.root === 'string' && payload.root.length > 0 ? payload.root : undefined
  const query = typeof payload?.query === 'string' ? payload.query : ''
  if (query.trim() === '') return ok({ ok: false, error: 'empty-query' })
  if (root !== undefined && !isAbsolute(root)) return ok({ ok: false, error: 'absolute-path-required' })

  const regexMode = payload.regex === true
  const caseSensitive = payload.caseSensitive === true
  let queryRe = null
  if (regexMode) {
    try {
      queryRe = new RegExp(query, caseSensitive ? '' : 'i')
    } catch {
      return ok({ ok: false, error: 'invalid-regex' })
    }
  }

  const include = compileGlobs(payload.include, false)
  const exclude = compileGlobs(payload.exclude, false)
  const excludeDirs = compileGlobs(payload.exclude, true)

  const dirBlocked = (rel) => {
    const dirRel = rel.endsWith('/') ? rel : `${rel}/`
    for (const { re } of excludeDirs) {
      if (re.test(rel) || re.test(dirRel)) return true
    }
    return false
  }
  const fileBlocked = (rel, base) => {
    for (const { re } of exclude) if (re.test(rel) || re.test(base)) return true
    if (include.length > 0) {
      let hit = false
      for (const { re } of include) if (re.test(rel) || re.test(base)) { hit = true; break }
      if (!hit) return true
    }
    return false
  }
  const matcher = { excludeDir: dirBlocked }

  let st
  try {
    st = root === undefined ? undefined : await stat(root)
  } catch {
    return ok({ ok: false, error: 'not-found' })
  }
  if (st !== undefined && !st.isDirectory()) return ok({ ok: false, error: 'not-a-directory' })

  const needle = caseSensitive ? query : query.toLowerCase()
  const hasNeedle = (text) => {
    if (queryRe !== null) {
      queryRe.lastIndex = 0
      return queryRe.test(text)
    }
    return (caseSensitive ? text : text.toLowerCase()).includes(needle)
  }

  const results = []
  let scanned = 0
  let truncatedFiles = false
  let truncatedMatches = false
  let totalMatches = 0
  const rootDir = root ?? process.cwd()

  for await (const file of walkFiles(rootDir, rootDir, matcher, new Set())) {
    const rel = relPath(rootDir, file)
    const name = basename(file)
    if (fileBlocked(rel, name)) continue
    scanned += 1
    if (scanned > config.maxFiles) {
      truncatedFiles = true
      break
    }
    const nameMatch = hasNeedle(name)
    let matches = null
    let fileStat
    try {
      fileStat = await stat(file)
    } catch {
      fileStat = undefined
    }
    if (fileStat !== undefined && fileStat.size <= config.maxFileBytes) {
      let buf
      try {
        buf = await readFile(file)
      } catch {
        buf = undefined
      }
      if (buf !== undefined && !looksBinary(buf)) {
        const lines = buf.toString('utf8').split('\n')
        const found = []
        for (let i = 0; i < lines.length; i++) {
          if (!hasNeedle(lines[i])) continue
          if (totalMatches >= config.maxMatches) {
            truncatedMatches = true
            break
          }
          totalMatches += 1
          found.push({
            line: i + 1,
            text: lines[i].length > config.maxLineLength
              ? `${lines[i].slice(0, config.maxLineLength)}…`
              : lines[i],
          })
        }
        if (found.length > 0) matches = found
      }
    }
    if (nameMatch || matches !== null) {
      results.push({ path: file, name, nameMatch, matches: matches ?? [] })
    }
  }

  results.sort((a, b) => {
    const am = a.matches.length > 0 ? 1 : 0
    const bm = b.matches.length > 0 ? 1 : 0
    if (am !== bm) return bm - am
    return a.path < b.path ? -1 : a.path > b.path ? 1 : 0
  })

  return ok({
    ok: true,
    query,
    root: root ?? process.cwd(),
    scanned,
    truncatedFiles,
    truncatedMatches,
    results,
  })
}

export function apply(ctx, config) {
  ctx.effect(() =>
    ctx.connection.rpc.handle(
      '/workspace-search',
      async (endpoint, payload, signal) => {
        try {
          if (endpoint === 'search') return await searchEndpoint(payload, config)
          return { ok: false, error: { code: 'bad-request', message: `unknown endpoint: ${endpoint}`, details: { issues: [] } } }
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error)
          return internalError(message)
        }
      },
      { authority: 'loopback' },
    ),
    'workspace-search: rpc channel',
  )
}
