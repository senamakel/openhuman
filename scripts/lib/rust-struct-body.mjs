/** Return the balanced Rust struct body following `marker`, or `null` if absent. */
export function rustStructBodyAfter(source, marker) {
  const markerAt = source.indexOf(marker);
  if (markerAt < 0) return null;
  const openAt = source.indexOf("{", markerAt + marker.length);
  if (openAt < 0) return null;

  let depth = 0;
  let inString = false;
  let escaped = false;
  let lineComment = false;
  let blockCommentDepth = 0;
  for (let index = openAt; index < source.length; index += 1) {
    const current = source[index];
    const next = source[index + 1];

    if (lineComment) {
      if (current === "\n") lineComment = false;
      continue;
    }
    if (blockCommentDepth > 0) {
      if (current === "/" && next === "*") {
        blockCommentDepth += 1;
        index += 1;
      } else if (current === "*" && next === "/") {
        blockCommentDepth -= 1;
        index += 1;
      }
      continue;
    }
    if (inString) {
      if (escaped) escaped = false;
      else if (current === "\\") escaped = true;
      else if (current === '"') inString = false;
      continue;
    }
    if (current === "/" && next === "/") {
      lineComment = true;
      index += 1;
    } else if (current === "/" && next === "*") {
      blockCommentDepth = 1;
      index += 1;
    } else if (current === '"') {
      inString = true;
    } else if (current === "{") {
      depth += 1;
    } else if (current === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(openAt + 1, index);
    }
  }
  throw new Error(`unclosed Rust struct after ${marker}`);
}
