import { For, Show, type JSX } from "solid-js";
import { commandRefClass, spinnerClass, spinnerTextClass, statusClass } from "./theme";
import type { InlineNode, OutputBlock, OutputDoc, SpinnerState } from "../generated/models";
import topologyImage from "../assets/topology.png";

// Logical asset keys (sent by the backend as Image.src) -> bundled, hashed URLs.
// Keeps the backend free of build-output paths.
const DOC_IMAGES: Record<string, string> = {
  topology: topologyImage,
};

type OutputRendererProps = {
  doc: OutputDoc;
  onRunCommand: (command: string) => void;
};

export function OutputRenderer(props: OutputRendererProps) {
  return (
    <div class="space-y-1">
      <For each={props.doc.blocks}>{(block) => renderBlock(block, props.onRunCommand)}</For>
    </div>
  );
}

function renderBlock(block: OutputBlock, onRunCommand: (command: string) => void): JSX.Element {
  if (block.kind === "heading") {
    return <div class="rb-heading-line rb-on-fg">{block.text}</div>;
  }

  if (block.kind === "paragraph") {
    return <div>{renderInlines(block.inlines, onRunCommand)}</div>;
  }

  if (block.kind === "list") {
    return (
      <div>
        <For each={block.items}>{(item) => <div class="rb-list-item">- {renderInlines(item, onRunCommand)}</div>}</For>
      </div>
    );
  }

  if (block.kind === "code") {
    return <pre class="rb-code-block">{block.text}</pre>;
  }

  if (block.kind === "status") {
    return <div class={statusClass(block.tone)}>{block.text}</div>;
  }

  if (block.kind === "image") {
    const src = DOC_IMAGES[block.src] ?? block.src;
    // The entry is scrolled to the bottom when appended, but an image has zero
    // height until it loads — so that scroll lands short. Re-scroll the output
    // container once the image's real height is known (e.g. the topology step).
    const scrollOutputToBottom = (img: HTMLImageElement) => {
      const scroller = img.closest(".rb-output-scroll");
      scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
    };
    return (
      <img
        class="rb-doc-image"
        src={src}
        alt={block.alt}
        onLoad={(event) => scrollOutputToBottom(event.currentTarget)}
      />
    );
  }

  if (block.kind === "entity_card") {
    return (
      <div class="rb-entity-card">
        <div class="rb-entity-card-header rb-on-fg">
          <span class="rb-entity-card-title">{block.title}</span>
          <Show when={block.subtitle}>
            {(subtitle) => <span class="rb-entity-card-subtitle"> — {subtitle()}</span>}
          </Show>
        </div>
        <div class="rb-entity-card-body">
          <For each={block.rows}>
            {(row) => (
              <div class="rb-entity-row">
                <span class="rb-entity-label">{row.label}</span>
                <span class="rb-entity-value">{row.value}</span>
              </div>
            )}
          </For>
          <Show when={block.body.length > 0}>
            <div class="rb-entity-card-prose">
              <For each={block.body}>{(child) => renderBlock(child, onRunCommand)}</For>
            </div>
          </Show>
        </div>
      </div>
    );
  }

  return (
    <div class={spinnerClass(block.state)}>
      <span class="rb-spinner-dot">{spinnerGlyph(block.state, block.text)}</span>
      <span class={spinnerTextClass(block.state)}>{spinnerMessage(block.state, block.text)}</span>
    </div>
  );
}

function spinnerGlyph(state: SpinnerState, text: string): string {
  if (state !== "running") {
    return "●";
  }

  const trimmed = text.trimStart();
  const first = trimmed.charAt(0);
  if (first && /[\u2800-\u28ff]/.test(first)) {
    return first;
  }

  return "⣾";
}

function spinnerMessage(state: SpinnerState, text: string): string {
  if (state !== "running") {
    return text;
  }

  const trimmed = text.trimStart();
  return trimmed.replace(/^[\u2800-\u28ff]\s*/, "");
}

function renderInlines(inlines: InlineNode[], onRunCommand: (command: string) => void): JSX.Element {
  return (
    <>
      <For each={inlines}>{(inline) => renderInline(inline, onRunCommand)}</For>
    </>
  );
}

function renderInline(inline: InlineNode, onRunCommand: (command: string) => void): JSX.Element {
  if (inline.kind === "text") {
    return <span>{inline.text}</span>;
  }

  if (inline.kind === "command_ref") {
    return (
      <button
        type="button"
        class={commandRefClass}
        onClick={() => {
          onRunCommand(inline.command);
        }}
      >
        {inline.label}
      </button>
    );
  }

  if (inline.kind === "emphasis") {
    return <em>{inline.text}</em>;
  }

  if (inline.kind === "strong") {
    return <strong>{inline.text}</strong>;
  }

  return <code>{inline.text}</code>;
}
