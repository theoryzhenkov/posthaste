import pino from "pino";

const logger = pino({
  level: import.meta.env.DEV ? "debug" : "info",
  browser: { asObject: true },
});

export const syncLogger = logger.child({ domain: "sync" });
export const uiLogger = logger.child({ domain: "ui" });
export const apiLogger = logger.child({ domain: "api" });

export default logger;
