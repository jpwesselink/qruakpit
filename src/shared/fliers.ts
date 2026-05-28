// Matches the upstream open-source build's free catalogue: one head, one plane
// colour. Extra characters and colours are an upstream Pro feature; this fork
// does not duplicate them.
export type Head = { id: string; name: string; sound: string };
export type PlaneColor = { id: string; name: string };

export const HEADS: Head[] = [{ id: "duck", name: "Duck", sound: "quack" }];
export const PLANE_COLORS: PlaneColor[] = [{ id: "red", name: "Red" }];

export function headById(id: string | undefined): Head {
  return HEADS.find((h) => h.id === id) ?? HEADS[0];
}

export function colorById(id: string | undefined): PlaneColor {
  return PLANE_COLORS.find((c) => c.id === id) ?? PLANE_COLORS[0];
}
