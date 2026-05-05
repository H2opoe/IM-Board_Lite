#!/usr/bin/env python3
"""Dump readable WeChat work-account text from message_fts.db.

This is a read-only fallback for cases where message_*.db keys are incomplete
but message/message_fts.db is readable.
"""

import argparse
import datetime as dt
import sqlite3
from collections import defaultdict

from wechat_cli.core.contacts import get_contact_names
from wechat_cli.core.context import AppContext
from wechat_cli.core.messages import parse_time_value


FTS_TABLES = (
    "message_fts_v4_0",
    "message_fts_v4_1",
    "message_fts_v4_2",
    "message_fts_v4_3",
    "message_fts_v4_4",
)


def _parse_args():
    parser = argparse.ArgumentParser(description="Dump work WeChat FTS text")
    parser.add_argument("--config", default="/Users/chase/.wechat-cli/config_work.json")
    parser.add_argument("--start-time", required=True)
    parser.add_argument("--end-time", required=True)
    parser.add_argument("--chat", default="", help="Optional chat name or username filter")
    parser.add_argument("--limit-per-chat", type=int, default=300)
    return parser.parse_args()


def _load_name_maps(app, conn):
    names = get_contact_names(app.cache, app.decrypted_dir)
    id_to_user = {
        row[0]: row[1]
        for row in conn.execute("select rowid, username from name2id")
    }
    return names, id_to_user


def _label(username, names):
    return names.get(username, username)


def main():
    args = _parse_args()
    start_ts = parse_time_value(args.start_time, "start_time")
    end_ts = parse_time_value(args.end_time, "end_time", is_end=True)
    app = AppContext(args.config)
    fts_path = app.cache.get("message/message_fts.db")
    if not fts_path:
        raise SystemExit("message/message_fts.db is not readable")

    conn = sqlite3.connect(fts_path)
    names, id_to_user = _load_name_maps(app, conn)
    grouped = defaultdict(list)

    for table in FTS_TABLES:
        try:
            rows = conn.execute(
                f"""
                select acontent, message_local_id, local_type, session_id,
                       sender_id, create_time
                from {table}
                where create_time >= ? and create_time <= ?
                order by create_time
                """,
                (start_ts, end_ts),
            )
        except sqlite3.Error:
            continue
        for content, local_id, local_type, session_id, sender_id, create_time in rows:
            if not content or not str(content).strip():
                continue
            chat_user = id_to_user.get(session_id, f"id:{session_id}")
            chat_name = _label(chat_user, names)
            if args.chat and args.chat not in (chat_user, chat_name):
                continue
            sender_user = id_to_user.get(sender_id, "") if sender_id else ""
            sender_name = _label(sender_user, names) if sender_user else "me/unknown"
            text = str(content).replace("\n", " / ")
            grouped[(chat_name, chat_user)].append(
                (create_time, sender_name, local_type, local_id, text)
            )

    print(
        f"Work WeChat FTS fallback: {args.start_time} ~ {args.end_time}, "
        f"{len(grouped)} chats"
    )
    for (chat_name, chat_user), items in sorted(
        grouped.items(), key=lambda item: (len(item[1]), item[1][-1][0]), reverse=True
    ):
        print(f"\n### {chat_name} | {chat_user} | {len(items)} text rows")
        for create_time, sender_name, local_type, local_id, text in items[: args.limit_per_chat]:
            stamp = dt.datetime.fromtimestamp(create_time).strftime("%Y-%m-%d %H:%M")
            if len(text) > 220:
                text = text[:220] + "..."
            print(f"[{stamp}] {sender_name}: {text}")


if __name__ == "__main__":
    main()
