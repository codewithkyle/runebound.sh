import type { OutputBlock, OutputDoc } from "./types";

export type ResolveCommandTarget = (candidate: string) => string | null;

export function parseOutputEntry(
  kind: "output" | "error" | "info" | "banner" | "spinner",
  text: string,
  resolveCommandTarget: ResolveCommandTarget
): OutputDoc {
  if (kind === "error") {
    return {
      blocks: [{ kind: "status", tone: "error", text }]
    };
  }

  if (kind === "info") {
    return {
      blocks: [{ kind: "status", tone: "info", text }]
    };
  }

  if (kind === "spinner") {
    const normalized = text.trim();
    const stateProbe = normalized.replace(/^[\u2800-\u28ff]\s*/, "");
    const lowered = stateProbe.toLowerCase();
    const state = lowered.startsWith("ok") ? "success" : lowered.startsWith("failed") ? "error" : "running";
    return {
      blocks: [{ kind: "spinner", state, text: normalized }]
    };
  }

  const blocks = parseMarkdownInspiredBlocks(text, resolveCommandTarget);
  return { blocks };
}

function parseMarkdownInspiredBlocks(text: string, resolveCommandTarget: ResolveCommandTarget): OutputBlock[] {
  const lines = text.split("\n");
  const blocks: OutputBlock[] = [];
  let paragraphLines: string[] = [];
  let listItems: string[] = [];

  const flushParagraph = () => {
    if (paragraphLines.length === 0) {
      return;
    }

    const paragraph = paragraphLines.join("\n");
    blocks.push({
      kind: "paragraph",
      inlines: parseInline(paragraph, resolveCommandTarget)
    });
    paragraphLines = [];
  };

  const flushList = () => {
    if (listItems.length === 0) {
      return;
    }

    blocks.push({
      kind: "list",
      items: listItems.map((item) => parseInline(item, resolveCommandTarget))
    });
    listItems = [];
  };

  for (const line of lines) {
    const trimmed = line.trim();

    const headingMatch = line.match(/^(#{1,3})\s+(.+)$/);
    if (headingMatch) {
      flushParagraph();
      flushList();
      blocks.push({
        kind: "heading",
        level: headingMatch[1].length,
        text: headingMatch[2].trim()
      });
      continue;
    }

    if (trimmed.length > 0) {
      const commandTarget = resolveCommandTarget(trimmed);
      if (commandTarget) {
        flushParagraph();
        listItems.push(trimmed);
        continue;
      }
    }

    const listMatch = line.match(/^\s*-\s+(.+)$/);
    if (listMatch) {
      flushParagraph();
      listItems.push(listMatch[1]);
      continue;
    }

    if (line.trim().length === 0) {
      flushParagraph();
      flushList();
      continue;
    }

    flushList();
    paragraphLines.push(line);
  }

  flushParagraph();
  flushList();
  return blocks;
}

function parseInline(text: string, resolveCommandTarget: ResolveCommandTarget) {
  const fastMatch = tryBuildSingleCommandInline(text, resolveCommandTarget);
  if (fastMatch) {
    return fastMatch;
  }

  const nodes: Array<
    | { kind: "text"; text: string }
    | { kind: "command_ref"; label: string; command: string }
    | { kind: "code"; text: string }
  > = [];
  const backtickRegex = /`([^`]+)`/g;
  let cursor = 0;
  let match: RegExpExecArray | null;

  while ((match = backtickRegex.exec(text)) !== null) {
    const start = match.index;
    const end = start + match[0].length;
    if (start > cursor) {
      nodes.push(...parseFreeText(text.slice(cursor, start), resolveCommandTarget));
    }

    const candidate = match[1].trim();
    const commandTarget = resolveCommandTarget(candidate);
    if (commandTarget) {
      nodes.push({ kind: "command_ref", label: commandTarget, command: commandTarget });
    } else {
      nodes.push({ kind: "code", text: candidate });
    }

    cursor = end;
  }

  if (cursor < text.length) {
    nodes.push(...parseFreeText(text.slice(cursor), resolveCommandTarget));
  }

  return nodes;
}

function tryBuildSingleCommandInline(text: string, resolveCommandTarget: ResolveCommandTarget) {
  const historyMatch = text.match(/^(\s*\d+:\s+)(.+?)\s*$/);
  if (historyMatch) {
    const candidate = historyMatch[2].trim();
    const commandTarget = resolveCommandTarget(candidate);
    if (commandTarget) {
      return [
        { kind: "text", text: historyMatch[1] } as const,
        { kind: "command_ref", label: candidate, command: commandTarget } as const
      ];
    }
  }

  const usageMatch = text.match(/^(\s*Usage:\s+)(.+?)\s*$/i);
  if (usageMatch) {
    const candidate = usageMatch[2].trim();
    const commandTarget = resolveCommandTarget(candidate);
    if (commandTarget) {
      return [
        { kind: "text", text: usageMatch[1] } as const,
        { kind: "command_ref", label: candidate, command: commandTarget } as const
      ];
    }
  }

  const commandTarget = resolveCommandTarget(text.trim());
  if (commandTarget) {
    return [{ kind: "command_ref", label: text.trim(), command: commandTarget } as const];
  }

  return null;
}

function parseFreeText(text: string, resolveCommandTarget: ResolveCommandTarget) {
  const commandKeywordRegex = /\b(?:type|run|use)\s+([a-z][a-z0-9-]*(?:\s+[a-z0-9][a-z0-9-]*){0,5})/gi;
  const nodes: Array<{ kind: "text"; text: string } | { kind: "command_ref"; label: string; command: string }> = [];
  let cursor = 0;
  let match: RegExpExecArray | null;

  while ((match = commandKeywordRegex.exec(text)) !== null) {
    const candidateRaw = match[1].trim();
    const candidate = normalizeEmbeddedCandidate(candidateRaw);
    const commandTarget = resolveCommandTarget(candidate);
    if (!commandTarget) {
      continue;
    }

    const start = match.index + match[0].indexOf(candidateRaw);
    const end = start + candidate.length;
    if (start > cursor) {
      nodes.push({ kind: "text", text: text.slice(cursor, start) });
    }
    nodes.push({ kind: "command_ref", label: candidate, command: commandTarget });
    cursor = end;
  }

  if (cursor < text.length) {
    nodes.push({ kind: "text", text: text.slice(cursor) });
  }

  return nodes;
}

function normalizeEmbeddedCandidate(candidateRaw: string): string {
  const tokens = candidateRaw.trim().split(/\s+/);
  if (tokens.length === 0) {
    return candidateRaw.trim();
  }

  const stopWords = new Set([
    "to",
    "for",
    "when",
    "then",
    "and",
    "or",
    "with",
    "without",
    "from",
    "in",
    "on",
    "at",
    "if",
    "while",
    "after",
    "before",
    "as",
    "again"
  ]);

  for (let i = 1; i < tokens.length; i += 1) {
    if (stopWords.has(tokens[i].toLowerCase())) {
      return tokens.slice(0, i).join(" ");
    }
  }

  return tokens.join(" ");
}
