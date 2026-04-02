export class ApiError extends Error {
  readonly status: number;
  readonly statusText: string;
  readonly code?: string;

  constructor(
    status: number,
    statusText: string,
    message?: string,
    code?: string,
  ) {
    super(message ?? `API error: ${status} ${statusText}`);
    this.name = "ApiError";
    this.status = status;
    this.statusText = statusText;
    this.code = code;
  }
}
