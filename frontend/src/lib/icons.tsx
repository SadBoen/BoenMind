import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

function Svg({ children, ...rest }: IconProps) {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {children}
    </svg>
  );
}

export const IconChat = (p: IconProps) => (
  <Svg {...p}>
    <path d="M21 15a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z" />
  </Svg>
);
export const IconCode = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="8 6 2 12 8 18" />
    <polyline points="16 6 22 12 16 18" />
  </Svg>
);
export const IconWiki = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
    <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
  </Svg>
);
export const IconGear = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09A1.65 1.65 0 0 0 15 4.6a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </Svg>
);
export const IconMenu = (p: IconProps) => (
  <Svg {...p}>
    <line x1="4" y1="7" x2="20" y2="7" />
    <line x1="4" y1="12" x2="20" y2="12" />
    <line x1="4" y1="17" x2="20" y2="17" />
  </Svg>
);
export const IconFiles = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </Svg>
);
export const IconDirTree = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 3h6l2 2h8v6H4z" />
    <path d="M4 11v10h16V11" />
    <path d="M8 15h4" />
    <path d="M8 19h8" />
  </Svg>
);
export const IconDrawer = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <line x1="15" y1="4" x2="15" y2="20" />
  </Svg>
);
export const IconPlus = (p: IconProps) => (
  <Svg {...p}>
    <line x1="12" y1="5" x2="12" y2="19" />
    <line x1="5" y1="12" x2="19" y2="12" />
  </Svg>
);
export const IconSearch = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="11" cy="11" r="7" />
    <line x1="21" y1="21" x2="16.65" y2="16.65" />
  </Svg>
);
export const IconMore = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="5" r="1.2" fill="currentColor" />
    <circle cx="12" cy="12" r="1.2" fill="currentColor" />
    <circle cx="12" cy="19" r="1.2" fill="currentColor" />
  </Svg>
);
export const IconSend = (p: IconProps) => (
  <Svg {...p}>
    <line x1="22" y1="2" x2="11" y2="13" />
    <polygon points="22 2 15 22 11 13 2 9 22 2" />
  </Svg>
);
export const IconStop = (p: IconProps) => (
  <Svg {...p}>
    <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
  </Svg>
);
export const IconCopy = (p: IconProps) => (
  <Svg {...p}>
    <rect x="9" y="9" width="13" height="13" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </Svg>
);
export const IconRetry = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="1 4 1 10 7 10" />
    <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
  </Svg>
);
export const IconEdit = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z" />
  </Svg>
);
export const IconPaperclip = (p: IconProps) => (
  <Svg {...p}>
    <path d="M21.44 11.05l-8.49 8.49a5.5 5.5 0 0 1-7.78-7.78l8.49-8.49a3.5 3.5 0 0 1 4.95 4.95l-8.49 8.49a1.5 1.5 0 1 1-2.12-2.12l7.78-7.78" />
  </Svg>
);
export const IconStar = (p: IconProps) => (
  <Svg {...p}>
    <polygon points="12 2 15 8.5 22 9.3 17 14.1 18.4 21 12 17.8 5.6 21 7 14.1 2 9.3 9 8.5 12 2" />
  </Svg>
);
export const IconStarFill = (p: IconProps) => (
  <Svg {...p}>
    <polygon points="12 2 15 8.5 22 9.3 17 14.1 18.4 21 12 17.8 5.6 21 7 14.1 2 9.3 9 8.5 12 2" fill="currentColor" />
  </Svg>
);
export const IconKey = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="8" cy="15" r="4" />
    <path d="M10.5 13.5 21 3" />
    <path d="M16 8h4" />
  </Svg>
);
export const IconServer = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="6" rx="1" />
    <rect x="3" y="14" width="18" height="6" rx="1" />
    <circle cx="7" cy="7" r="1" fill="currentColor" />
    <circle cx="7" cy="17" r="1" fill="currentColor" />
  </Svg>
);
export const IconRefresh = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="23 4 23 10 17 10" />
    <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
  </Svg>
);
export const IconPlug = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 22v-5" />
    <path d="M8 2v5" />
    <path d="M16 2v5" />
    <path d="M8 7h8a4 4 0 0 1 4 4v2a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4v-2a4 4 0 0 1 4-4z" />
  </Svg>
);
export const IconWifi = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5 10a11 11 0 0 1 14.08 0" />
    <path d="M8.5 13.7a6 6 0 0 1 7 0" />
    <path d="M12 17.7h.01" />
  </Svg>
);
export const IconWifiOff = (p: IconProps) => (
  <Svg {...p}>
    <line x1="1" y1="1" x2="23" y2="23" />
    <path d="M16.72 9A10.94 10.94 0 0 1 19 10.5" />
    <path d="M5 10.5a10.94 10.94 0 0 1 5.17-2.39" />
    <path d="M8.53 14.1a6 6 0 0 1 6.95 0" />
    <path d="M12 17.7h.01" />
  </Svg>
);
export const IconFolder = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
  </Svg>
);
export const IconFile = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </Svg>
);
export const IconImage = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <circle cx="8.5" cy="8.5" r="1.5" />
    <polyline points="21 15 16 10 5 21" />
  </Svg>
);
export const IconChevron = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="6 9 12 15 18 9" />
  </Svg>
);
export const IconClose = (p: IconProps) => (
  <Svg {...p}>
    <line x1="18" y1="6" x2="6" y2="18" />
    <line x1="6" y1="6" x2="18" y2="18" />
  </Svg>
);
export const IconTrash = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="3 6 5 6 21 6" />
    <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
    <path d="M10 11v6" />
    <path d="M14 11v6" />
  </Svg>
);
export const IconUser = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="8" r="4" />
    <path d="M4 20c0-4 4-6 8-6s8 2 8 6" />
  </Svg>
);
export const IconBot = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="8" width="16" height="10" rx="3" />
    <circle cx="9" cy="13" r="1" fill="currentColor" />
    <circle cx="15" cy="13" r="1" fill="currentColor" />
    <path d="M12 8V5" />
    <circle cx="12" cy="4" r="1" />
  </Svg>
);
export const IconBrain = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 4a4 4 0 0 0-4 4v.2A3.5 3.5 0 0 0 5 11.5V13a3 3 0 0 0 3 3h.2A3.5 3.5 0 0 0 12 19.5 3.5 3.5 0 0 0 15.8 16H16a3 3 0 0 0 3-3v-1.5A3.5 3.5 0 0 0 16 8.2V8a4 4 0 0 0-4-4z" />
  </Svg>
);
export const IconCheck = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="20 6 9 17 4 12" />
  </Svg>
);
export const IconTag = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" />
    <circle cx="7" cy="7" r="1.2" fill="currentColor" />
  </Svg>
);
export const IconArchive = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="3" width="18" height="5" rx="1" />
    <path d="M5 8v11a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8" />
    <line x1="10" y1="12" x2="14" y2="12" />
  </Svg>
);
export const IconSave = (p: IconProps) => (
  <Svg {...p}>
    <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
    <polyline points="17 21 17 13 7 13 7 21" />
    <polyline points="7 3 7 8 15 8" />
  </Svg>
);
export const IconUpload = (p: IconProps) => (
  <Svg {...p}>
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <polyline points="17 8 12 3 7 8" />
    <line x1="12" y1="3" x2="12" y2="15" />
  </Svg>
);
export const IconDownload = (p: IconProps) => (
  <Svg {...p}>
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <polyline points="7 10 12 15 17 10" />
    <line x1="12" y1="15" x2="12" y2="3" />
  </Svg>
);
export const IconPath = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="6" cy="6" r="2" />
    <circle cx="18" cy="18" r="2" />
    <path d="M8 7.5 16 16.5" />
  </Svg>
);
export const IconRename = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z" />
  </Svg>
);
export const IconUninstall = (p: IconProps) => (
  <Svg {...p}>
    <polyline points="3 6 5 6 21 6" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
  </Svg>
);
export const IconEmptyChat = (p: IconProps) => (
  <Svg width="48" height="48" {...p}>
    <path d="M21 15a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z" />
    <line x1="8" y1="10" x2="16" y2="10" />
    <line x1="8" y1="14" x2="13" y2="14" />
  </Svg>
);
