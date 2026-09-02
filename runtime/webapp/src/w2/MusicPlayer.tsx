import { useCallback, useEffect, useRef, useState } from "react";
import {
  Folder,
  ListMusic,
  Music,
  Pause,
  Play,
  RotateCcw,
  Search,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
} from "lucide-react";
import { api, type FsEntry } from "./api";

type Track = {
  id: string;
  title: string;
  artist: string;
  path: string;
  size: number | null;
};

export function MusicPlayer() {
  const [tracks, setTracks] = useState<Track[]>([]);
  const [currentIndex, setCurrentIndex] = useState<number>(-1);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(0.8);
  const [isMuted, setIsMuted] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [activeTab, setActiveTab] = useState<"library" | "playlist">("library");
  const [playlist, setPlaylist] = useState<Track[]>([]);

  const audioRef = useRef<HTMLAudioElement | null>(null);

  // 扫描工作区内的音频文件 (.mp3, .wav, .ogg, .flac, .m4a, .aac)
  const scanWorkspaceAudio = useCallback(async () => {
    try {
      const res = await api.fs.list("");
      const audioExts = [".mp3", ".wav", ".ogg", ".flac", ".m4a", ".aac"];
      const audioFiles: Track[] = (res.entries ?? [])
        .filter((e) => e.kind === "file" && audioExts.some((ext) => e.name.toLowerCase().endsWith(ext)))
        .map((e, idx) => {
          const stem = e.name.replace(/\.[^/.]+$/, "");
          let artist = "本地音频";
          let title = stem;
          if (stem.includes(" - ")) {
            const parts = stem.split(" - ");
            artist = parts[0];
            title = parts.slice(1).join(" - ");
          }
          return {
            id: `track-${idx}-${e.name}`,
            title,
            artist,
            path: e.name,
            size: e.size,
          };
        });

      // 如果工作区暂无音频，提供内置示例曲目
      if (audioFiles.length === 0) {
        const demoTracks: Track[] = [
          {
            id: "demo-1",
            title: "BoenMind Ambient Theme",
            artist: "Runtime Acoustic",
            path: "ambient.mp3",
            size: 3450000,
          },
          {
            id: "demo-2",
            title: "Synthetic Focus (Lofi Flow)",
            artist: "AI Soundscape",
            path: "focus_lofi.mp3",
            size: 4200000,
          },
        ];
        setTracks(demoTracks);
        setPlaylist(demoTracks);
      } else {
        setTracks(audioFiles);
        setPlaylist(audioFiles);
      }
    } catch {
      // 容错处理
    }
  }, []);

  useEffect(() => {
    void scanWorkspaceAudio();
  }, [scanWorkspaceAudio]);

  const currentTrack = currentIndex >= 0 && currentIndex < playlist.length ? playlist[currentIndex] : null;

  const togglePlay = () => {
    if (!audioRef.current || !currentTrack) {
      if (playlist.length > 0) {
        setCurrentIndex(0);
        setIsPlaying(true);
      }
      return;
    }
    if (isPlaying) {
      audioRef.current.pause();
      setIsPlaying(false);
    } else {
      void audioRef.current.play().then(() => setIsPlaying(true)).catch(() => setIsPlaying(false));
    }
  };

  const playTrack = (index: number) => {
    setCurrentIndex(index);
    setIsPlaying(true);
  };

  const nextTrack = () => {
    if (playlist.length === 0) return;
    const next = (currentIndex + 1) % playlist.length;
    setCurrentIndex(next);
    setIsPlaying(true);
  };

  const prevTrack = () => {
    if (playlist.length === 0) return;
    const prev = (currentIndex - 1 + playlist.length) % playlist.length;
    setCurrentIndex(prev);
    setIsPlaying(true);
  };

  const handleTimeUpdate = () => {
    if (audioRef.current) {
      setCurrentTime(audioRef.current.currentTime);
      setDuration(audioRef.current.duration || 0);
    }
  };

  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    const time = Number(e.target.value);
    setCurrentTime(time);
    if (audioRef.current) {
      audioRef.current.currentTime = time;
    }
  };

  const formatTime = (secs: number) => {
    if (isNaN(secs) || secs < 0) return "0:00";
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m}:${s < 10 ? "0" : ""}${s}`;
  };

  const filteredTracks = tracks.filter(
    (t) =>
      t.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      t.artist.toLowerCase().includes(searchQuery.toLowerCase()) ||
      t.path.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="flex h-full flex-col bg-background/50 text-[13px]" data-slot="music-player">
      {/* 隐藏的 HTML5 Audio 标签 */}
      <audio
        ref={audioRef}
        src={currentTrack ? api.fs.downloadUrl(currentTrack.path) : undefined}
        onTimeUpdate={handleTimeUpdate}
        onEnded={nextTrack}
      />

      {/* 顶部标签与搜索 */}
      <div className="flex flex-col gap-2 p-3 border-b border-border/40">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-1.5 font-medium text-foreground">
            <Music size={16} className="text-primary" />
            <span>音乐播放器</span>
          </div>
          <div className="flex items-center gap-1 bg-muted/40 p-0.5 rounded-lg border border-border/40 text-[11.5px]">
            <button
              className={`px-2 py-0.5 rounded-md transition-colors ${activeTab === "library" ? "bg-background shadow-xs font-medium text-foreground" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setActiveTab("library")}
            >
              曲库
            </button>
            <button
              className={`px-2 py-0.5 rounded-md transition-colors ${activeTab === "playlist" ? "bg-background shadow-xs font-medium text-foreground" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setActiveTab("playlist")}
            >
              播放列表 ({playlist.length})
            </button>
          </div>
        </div>

        <div className="relative">
          <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <input
            type="text"
            placeholder="搜索曲目、艺术家…"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full rounded-md border border-border/50 bg-background/80 pl-8 pr-3 py-1 text-[12px] placeholder:text-muted-foreground/70 focus:outline-none focus:ring-1 focus:ring-primary"
          />
        </div>
      </div>

      {/* 曲目列表 */}
      <div className="flex-1 overflow-y-auto p-2 space-y-1">
        {(activeTab === "library" ? filteredTracks : playlist).length === 0 ? (
          <div className="flex flex-col items-center justify-center py-10 text-muted-foreground text-[12px] gap-1.5">
            <Music size={24} className="opacity-40" />
            <span>暂无匹配曲目 (可将音频放入工作区)</span>
          </div>
        ) : (
          (activeTab === "library" ? filteredTracks : playlist).map((track, idx) => {
            const isSelected = currentTrack?.id === track.id;
            return (
              <div
                key={track.id}
                onClick={() => playTrack(idx)}
                className={`group flex items-center justify-between gap-2 p-2 rounded-lg cursor-pointer transition-all ${isSelected ? "bg-primary/10 border border-primary/20 text-primary" : "hover:bg-muted/40 border border-transparent"}`}
              >
                <div className="flex items-center gap-2.5 min-w-0">
                  <div
                    className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-md ${isSelected ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground group-hover:bg-background"}`}
                  >
                    {isSelected && isPlaying ? <Pause size={13} /> : <Play size={13} className="ml-0.5" />}
                  </div>
                  <div className="min-w-0">
                    <div className="truncate font-medium leading-tight">{track.title}</div>
                    <div className="truncate text-[11px] text-muted-foreground/80">{track.artist}</div>
                  </div>
                </div>
                <div className="text-[11px] font-mono text-muted-foreground shrink-0">
                  {track.size ? `${(track.size / 1024 / 1024).toFixed(1)}MB` : "Audio"}
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* 底部播放控制面板 */}
      <div className="border-t border-border/50 bg-background/90 backdrop-blur-md p-3 flex flex-col gap-2">
        {/* 进度条 */}
        <div className="flex items-center gap-2 text-[11px] font-mono text-muted-foreground">
          <span>{formatTime(currentTime)}</span>
          <input
            type="range"
            min={0}
            max={duration || 100}
            value={currentTime}
            onChange={handleSeek}
            className="flex-1 h-1 bg-muted rounded-lg appearance-none cursor-pointer accent-primary"
          />
          <span>{formatTime(duration)}</span>
        </div>

        {/* 主控制按钮 */}
        <div className="flex items-center justify-between">
          <div className="min-w-0 max-w-[120px]">
            <div className="truncate text-[12px] font-medium leading-tight">
              {currentTrack?.title || "未选择播放曲目"}
            </div>
            <div className="truncate text-[10.5px] text-muted-foreground">
              {currentTrack?.artist || "点击曲目开始播放"}
            </div>
          </div>

          <div className="flex items-center gap-1.5">
            <button
              onClick={prevTrack}
              className="p-1.5 rounded-full hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
              title="上一曲"
            >
              <SkipBack size={15} />
            </button>
            <button
              onClick={togglePlay}
              className="p-2 rounded-full bg-primary text-primary-foreground hover:opacity-90 transition-opacity shadow-xs"
              title={isPlaying ? "暂停" : "播放"}
            >
              {isPlaying ? <Pause size={16} /> : <Play size={16} className="ml-0.5" />}
            </button>
            <button
              onClick={nextTrack}
              className="p-1.5 rounded-full hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
              title="下一曲"
            >
              <SkipForward size={15} />
            </button>
          </div>

          <div className="flex items-center gap-1.5 text-muted-foreground">
            <button
              onClick={() => {
                if (audioRef.current) {
                  const nextMute = !isMuted;
                  setIsMuted(nextMute);
                  audioRef.current.muted = nextMute;
                }
              }}
              className="hover:text-foreground"
            >
              {isMuted ? <VolumeX size={15} /> : <Volume2 size={15} />}
            </button>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={isMuted ? 0 : volume}
              onChange={(e) => {
                const v = Number(e.target.value);
                setVolume(v);
                setIsMuted(false);
                if (audioRef.current) {
                  audioRef.current.volume = v;
                  audioRef.current.muted = false;
                }
              }}
              className="w-14 h-1 bg-muted rounded-lg appearance-none cursor-pointer accent-primary"
            />
          </div>
        </div>
      </div>
    </div>
  );
}
