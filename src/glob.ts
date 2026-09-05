/**
 * Glob matching for DataCore record paths.
 *
 * The same semantics StarBreaker's extraction filter uses, so a domain's
 * declared filters mean the same thing whether they are selecting records out of
 * the archive or selecting them out of the loaded pool:
 *
 *   `*`  matches within one path segment and does not cross a `/`
 *   `**` matches across segments
 *   `?`  matches a single character
 *
 * This matters because a domain is handed whatever the extraction produced,
 * which includes records pulled in for *other* domains. Without scoping each
 * domain to its own filters, registering a new domain silently widens every
 * existing domain's input.
 */

function escapeLiteral(text: string): string {
  return text.replace(/[.+^${}()|[\]\\]/g, "\\$&");
}

/** Compile a glob to an anchored, case-insensitive regular expression. */
export function globToRegExp(glob: string): RegExp {
  let pattern = "";

  for (let i = 0; i < glob.length; i += 1) {
    const char = glob[i];

    if (char === "*") {
      if (glob[i + 1] === "*") {
        // `**/` should also match zero directories, so `**/x` matches a bare `x`.
        if (glob[i + 2] === "/") {
          pattern += "(?:.*/)?";
          i += 2;
        } else {
          pattern += ".*";
          i += 1;
        }
      } else {
        pattern += "[^/]*";
      }
      continue;
    }

    pattern += char === "?" ? "[^/]" : escapeLiteral(char);
  }

  return new RegExp(`^${pattern}$`, "i");
}

/** True when `path` matches any of the globs. An empty list matches everything. */
export function matchesAny(path: string, globs: string[]): boolean {
  if (globs.length === 0) return true;
  return globs.some((glob) => globToRegExp(glob).test(path));
}
