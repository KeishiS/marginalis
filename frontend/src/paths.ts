export function externalPath(basePath: string, path: string): string {
  const base = basePath === "/" ? "" : basePath.replace(/\/$/, "");
  return `${base}${path}`;
}
