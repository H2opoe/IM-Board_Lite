function parseLocalDateTime(value: string) {
  const match = value.match(
    /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?/
  );
  if (!match) {
    const fallback = new Date(value);
    return Number.isNaN(fallback.getTime()) ? null : fallback;
  }

  const [, year, month, day, hour = "00", minute = "00", second = "00"] = match;
  return new Date(
    Number(year),
    Number(month) - 1,
    Number(day),
    Number(hour),
    Number(minute),
    Number(second)
  );
}

function startOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function startOfWeek(date: Date) {
  const day = date.getDay() || 7;
  const start = startOfDay(date);
  start.setDate(start.getDate() - day + 1);
  return start;
}

function pad(value: number) {
  return String(value).padStart(2, "0");
}

function clock(date: Date) {
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatRelativeDateTime(value: string, now = new Date()) {
  const date = parseLocalDateTime(value);
  if (!date) return value;

  const currentDay = startOfDay(now);
  const targetDay = startOfDay(date);
  const dayDiff = Math.round((currentDay.getTime() - targetDay.getTime()) / 86_400_000);

  if (dayDiff === 0) return `今天 ${clock(date)}`;
  if (dayDiff === 1) return `昨天 ${clock(date)}`;

  if (date >= startOfWeek(now) && date <= now) {
    const weekDay = ["日", "一", "二", "三", "四", "五", "六"][date.getDay()];
    return `本周${weekDay} ${clock(date)}`;
  }

  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${clock(date)}`;
}
