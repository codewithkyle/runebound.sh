export type OutputDoc = {
  blocks: OutputBlock[];
};

export type OutputBlock =
  | {
      kind: "heading";
      level: number;
      text: string;
    }
  | {
      kind: "paragraph";
      inlines: InlineNode[];
    }
  | {
      kind: "list";
      items: InlineNode[][];
    }
  | {
      kind: "code";
      language?: string | null;
      text: string;
    }
  | {
      kind: "status";
      tone: StatusTone;
      text: string;
    }
  | {
      kind: "spinner";
      state: SpinnerState;
      text: string;
    }
  | {
      kind: "entity_card";
      title: string;
      rows: EntityCardRow[];
    };

export type EntityCardRow = {
  label: string;
  value: string;
};

export type InlineNode =
  | {
      kind: "text";
      text: string;
    }
  | {
      kind: "command_ref";
      label: string;
      command: string;
    }
  | {
      kind: "emphasis";
      text: string;
    }
  | {
      kind: "strong";
      text: string;
    }
  | {
      kind: "code";
      text: string;
    };

export type StatusTone = "success" | "info" | "warning" | "error";

export type SpinnerState = "running" | "success" | "error";
