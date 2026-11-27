/** Types for `health-strip.mjs`. */

export interface CellGeometry {
  y: number;
  height: number;
}

export function cellGeometry(failed: boolean, height: number): CellGeometry;
