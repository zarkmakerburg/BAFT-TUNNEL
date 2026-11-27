/**
 * Geometry of one health-strip cell. A failed probe is shorter than a successful one, so up/down
 * is readable without relying on colour (the strip's aria-label still carries the count).
 */
export function cellGeometry(failed, height) {
  if (!failed) return { y: 0, height };
  return { y: Math.round(height * 0.35), height: Math.round(height * 0.65) };
}
