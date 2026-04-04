const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const DATE_FORMATTER = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
});

/**
 * Formats a date as a relative time string.
 * - Under 1 minute: "just now"
 * - Under 1 hour: "X min ago"
 * - Under 24 hours: "X hours ago"
 * - Under 7 days: "X days ago"
 * - Otherwise: formatted date (e.g., "Mar 25")
 *
 * @spec docs/L1-ui#messagelist
 */
export function formatRelativeTime(isoDate: string): string {
  const date = new Date(isoDate);
  const now = new Date();
  const diffSeconds = Math.floor((now.getTime() - date.getTime()) / 1000);

  if (diffSeconds < MINUTE) {
    return "just now";
  }
  if (diffSeconds < HOUR) {
    const minutes = Math.floor(diffSeconds / MINUTE);
    return `${minutes} min ago`;
  }
  if (diffSeconds < DAY) {
    const hours = Math.floor(diffSeconds / HOUR);
    return `${hours} ${hours === 1 ? "hour" : "hours"} ago`;
  }
  if (diffSeconds < 7 * DAY) {
    const days = Math.floor(diffSeconds / DAY);
    return `${days} ${days === 1 ? "day" : "days"} ago`;
  }

  return DATE_FORMATTER.format(date);
}
