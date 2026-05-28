// Banner typography. Matches the upstream open-source build: system font stacks
// only, no web fonts (stays offline; no vendored woff2).
export type FontChoice = { id: string; name: string; stack: string };

export const FONTS: FontChoice[] = [
  {
    id: "system",
    name: "System",
    stack: "-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif",
  },
  {
    id: "rounded",
    name: "Rounded",
    stack:
      "'SF Pro Rounded', 'Arial Rounded MT Bold', 'Hiragino Maru Gothic ProN', system-ui, sans-serif",
  },
  { id: "serif", name: "Serif", stack: "Georgia, 'Times New Roman', serif" },
  { id: "mono", name: "Mono", stack: "'SF Mono', Menlo, Consolas, monospace" },
  { id: "condensed", name: "Condensed", stack: "'Arial Narrow', 'Helvetica Neue', sans-serif" },
];

export function fontById(id: string | undefined): FontChoice {
  return FONTS.find((f) => f.id === id) ?? FONTS[0];
}
