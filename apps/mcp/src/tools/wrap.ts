import { ApiError, ConnectionError } from "../client.js";

export type ToolWrapper = <Args>(
  fn: (args: Args) => Promise<unknown>,
) => (args: Args) => Promise<{
  content: { type: "text"; text: string }[];
  isError?: true;
}>;

export const wrapTool: ToolWrapper =
  <Args>(fn: (args: Args) => Promise<unknown>) =>
  async (args: Args) => {
    try {
      const result = await fn(args);
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify(result ?? null, null, 2),
          },
        ],
      };
    } catch (error) {
      const message =
        error instanceof ApiError || error instanceof ConnectionError
          ? error.message
          : error instanceof Error
            ? error.message
            : String(error);
      return {
        isError: true,
        content: [{ type: "text" as const, text: message }],
      };
    }
  };
