function parseLocalDateTime(value: string) {
  const text = value.trim();
  const match = text.match(
    /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2})(?:\.(\d+))?)?(Z|[+-]\d{2}:?\d{2})?)?$/i
  );
  if (!match) {
    // Do not accept a valid date prefix followed by an invalid time or suffix.
    if (/^\d{4}-/.test(text)) return null;
    const fallback = new Date(text);
    return Number.isNaN(fallback.getTime()) ? null : fallback;
  }

  const [, yearText, monthText, dayText, hourText = "00", minuteText = "00", secondText = "00", fraction = "", zone] = match;
  const year = Number(yearText), month = Number(monthText), day = Number(dayText);
  const hour = Number(hourText), minute = Number(minuteText), second = Number(secondText);
  const millisecond = Number(fraction.slice(0, 3).padEnd(3, "0"));
  const calendar = new Date(0);
  calendar.setUTCFullYear(year, month - 1, day);
  calendar.setUTCHours(0, 0, 0, 0);
  if (calendar.getUTCFullYear() !== year || calendar.getUTCMonth() !== month - 1 || calendar.getUTCDate() !== day) return null;
  if (hour > 24 || minute > 59 || second > 59) return null;
  if (hour === 24 && (minute !== 0 || second !== 0 || /[1-9]/.test(fraction))) return null;

  if (zone) {
    let offsetMinutes = 0;
    if (zone.toUpperCase() !== "Z") {
      const offsetHour = Number(zone.slice(1, 3));
      const offsetMinute = Number(zone.slice(-2));
      if (offsetHour > 23 || offsetMinute > 59) return null;
      offsetMinutes = (zone[0] === "+" ? 1 : -1) * (offsetHour * 60 + offsetMinute);
    }
    calendar.setUTCHours(hour, minute, second, millisecond);
    return new Date(calendar.getTime() - offsetMinutes * 60_000);
  }

  // SQLite date/time values without a timezone retain local wall-clock meaning.
  const local = new Date(0);
  local.setFullYear(year, month - 1, day);
  local.setHours(hour, minute, second, millisecond);
  return local;
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

export function formatLocalDate(date: Date) {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
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

  return `${formatLocalDate(date)} ${clock(date)}`;
}
