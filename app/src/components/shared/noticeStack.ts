export interface StackNotice {
  id: string;
}

export function upsertNoticeToStackTop<TNotice extends StackNotice>(notices: TNotice[], notice: TNotice) {
  const remainingNotices = notices.filter((currentNotice) => currentNotice.id !== notice.id);
  return [...remainingNotices, notice];
}
