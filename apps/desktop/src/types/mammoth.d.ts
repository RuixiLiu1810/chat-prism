declare module "mammoth" {
  export interface MammothMessage {
    type: string;
    message: string;
  }

  export interface ConvertResult {
    value: string;
    messages: MammothMessage[];
  }

  export interface ConvertOptions {
    includeDefaultStyleMap?: boolean;
    includeEmbeddedStyleMap?: boolean;
    styleMap?: string[];
  }

  export function convertToHtml(
    input: { arrayBuffer: ArrayBuffer },
    options?: ConvertOptions,
  ): Promise<ConvertResult>;
}
