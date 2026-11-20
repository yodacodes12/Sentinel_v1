// One consistent icon set: 16px grid, 1.5px strokes, no fills.
import type { ReactNode } from "react";

const PATHS = {
  logo: (
    <>
      <path d="M8 1.5 13.5 3.6v4.2c0 3.2-2.2 5.6-5.5 6.7C4.7 13.4 2.5 11 2.5 7.8V3.6Z" />
      <circle cx="8" cy="7.6" r="1.9" />
      <path d="M8 5.7V4.2M8 11V9.5M6.1 7.6H4.6M11.4 7.6H9.9" />
    </>
  ),
  overview: (
    <>
      <rect x="2" y="2" width="5" height="5" />
      <rect x="9" y="2" width="5" height="5" />
      <rect x="2" y="9" width="5" height="5" />
      <rect x="9" y="9" width="5" height="5" />
    </>
  ),
  cpu: (
    <>
      <rect x="4" y="4" width="8" height="8" />
      <rect x="6.3" y="6.3" width="3.4" height="3.4" />
      <path d="M6 4V2M10 4V2M6 14v-2M10 14v-2M4 6H2M4 10H2M14 6h-2M14 10h-2" />
    </>
  ),
  board: (
    <>
      <rect x="2" y="2.5" width="12" height="11" />
      <rect x="5" y="5" width="3.5" height="3.5" />
      <path d="M10.5 5v4M12 5v4M4.5 11h6" />
    </>
  ),
  activity: <path d="M1.5 8.5h3l1.8-5 3 9 1.9-4h3.3" />,
  library: (
    <>
      <path d="M3 2.5h4v11H3zM7 2.5h3v11H7z" />
      <path d="m10.6 4 2.6-.7 2 9.6-2.6.6" transform="translate(-1.6 .2)" />
    </>
  ),
  diagnostics: (
    <>
      <circle cx="7" cy="7" r="4.5" />
      <path d="m10.4 10.4 3.6 3.6M5 7h1.2l.8-1.6L8 8.6 8.7 7H9.4" />
    </>
  ),
  utilities: (
    <>
      <rect x="2" y="3" width="12" height="10" />
      <path d="m4.5 6.5 2 1.7-2 1.7M8 10h3.5" />
    </>
  ),
  services: (
    <>
      <path d="M8 2 14 5 8 8 2 5Z" />
      <path d="m2 8 6 3 6-3M2 11l6 3 6-3" />
    </>
  ),
  bell: (
    <>
      <path d="M4 11V7.5a4 4 0 0 1 8 0V11l1.2 1.5H2.8Z" />
      <path d="M6.7 14h2.6" />
    </>
  ),
  settings: (
    <>
      <path d="M2 4.5h7M12 4.5h2M2 11.5h2M7 11.5h7" />
      <circle cx="10.5" cy="4.5" r="1.5" />
      <circle cx="5.5" cy="11.5" r="1.5" />
    </>
  ),
  memory: (
    <>
      <rect x="1.5" y="4.5" width="13" height="6" />
      <path d="M4 6.5v2M6.5 6.5v2M9 6.5v2M11.5 6.5v2M3 10.5v1.5M6 10.5v1.5M10 10.5v1.5M13 10.5v1.5" />
    </>
  ),
  storage: (
    <>
      <rect x="2.5" y="2" width="11" height="12" />
      <circle cx="8" cy="7" r="2.6" />
      <path d="M5 12h1.5" />
    </>
  ),
  gpu: (
    <>
      <rect x="1.5" y="4" width="13" height="7" />
      <circle cx="5.5" cy="7.5" r="1.8" />
      <circle cx="10.5" cy="7.5" r="1.8" />
      <path d="M3 11v2M6 11v2" />
    </>
  ),
  battery: (
    <>
      <rect x="1.5" y="4.5" width="11.5" height="7" />
      <path d="M13 6.5h1.5v3H13" />
    </>
  ),
  network: (
    <>
      <rect x="6" y="2" width="4" height="3.5" />
      <rect x="1.5" y="10.5" width="4" height="3.5" />
      <rect x="10.5" y="10.5" width="4" height="3.5" />
      <path d="M8 5.5V8M3.5 10.5V8h9v2.5" />
    </>
  ),
  power: (
    <>
      <path d="M8 1.8v6" />
      <path d="M4.4 4.2a5.2 5.2 0 1 0 7.2 0" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 7.2v4M8 4.8v.6" />
    </>
  ),
  warning: (
    <>
      <path d="M8 2 14.5 13.5h-13Z" />
      <path d="M8 6.5v3.4M8 11.4v.5" />
    </>
  ),
  minimize: <path d="M3.5 8h9" />,
  maximize: <rect x="3.5" y="3.5" width="9" height="9" />,
  close: <path d="m4 4 8 8M12 4l-8 8" />,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      {PATHS[name]}
    </svg>
  );
}
