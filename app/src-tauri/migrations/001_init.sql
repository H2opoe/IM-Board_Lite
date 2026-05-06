pragma journal_mode = WAL;
pragma foreign_keys = ON;

create table if not exists app_meta (
  key text primary key,
  value text not null,
  updated_at text not null
);

create table if not exists profiles (
  id text primary key,
  platform text not null check(platform in ('wechat', 'wecom', 'feishu', 'dingtalk')),
  label text not null,
  enabled integer not null default 1,
  config_json text not null default '{}',
  status text not null default 'normal',
  sort_order integer not null default 0,
  created_at text not null,
  updated_at text not null
);

create table if not exists sync_state (
  profile_id text not null,
  day text not null,
  last_sync_at text,
  last_analysis_at text,
  cursor_json text not null default '{}',
  updated_at text not null,
  primary key(profile_id, day)
);

create table if not exists daily_messages (
  id text primary key,
  day text not null,
  profile_id text not null,
  platform text not null,
  chat_id text not null,
  chat_name text not null,
  is_group integer not null default 0,
  sender_id text not null,
  sender_name text not null,
  timestamp integer not null,
  time_text text not null,
  msg_type text not null,
  content text not null,
  raw_type text,
  local_id text,
  raw_json text,
  content_hash text not null,
  analyzed_at text,
  topic_summarized_at text,
  partial integer not null default 0
);

create unique index if not exists idx_daily_messages_local
on daily_messages(profile_id, platform, chat_id, local_id)
where local_id is not null and local_id <> '';

create unique index if not exists idx_daily_messages_hash
on daily_messages(profile_id, platform, chat_id, timestamp, sender_id, content_hash);

create index if not exists idx_daily_messages_day_profile on daily_messages(day, profile_id);
create index if not exists idx_daily_messages_chat on daily_messages(day, chat_id);
create index if not exists idx_daily_messages_profile_chat_time
on daily_messages(day, profile_id, platform, chat_id, timestamp);

create table if not exists action_items (
  id text primary key,
  type text not null check(type in ('task', 'reply', 'attention')),
  status text not null check(status in ('open', 'done', 'ignored')),
  priority text not null check(priority in ('high', 'medium', 'low')),
  title text not null,
  description text not null,
  suggested_reply text,
  profile_id text not null,
  platform text not null,
  chat_id text not null,
  chat_name text not null,
  source_message_ids text not null default '[]',
  evidence_summary text not null default '',
  context_incomplete integer not null default 0,
  carry_over integer not null default 1,
  first_detected_at text not null,
  last_updated_at text not null,
  completed_at text
);

create index if not exists idx_action_items_status_type on action_items(status, type);
create index if not exists idx_action_items_profile on action_items(profile_id);

create table if not exists daily_stats (
  id text primary key,
  day text not null,
  profile_id text not null,
  metric text not null,
  value_json text not null,
  updated_at text not null
);

create unique index if not exists idx_daily_stats_unique
on daily_stats(day, profile_id, metric);

create table if not exists ai_analysis_runs (
  id text primary key,
  day text not null,
  profile_id text not null,
  input_message_ids text not null default '[]',
  status text not null check(status in ('pending', 'running', 'done', 'failed')),
  model text,
  token_usage_json text,
  diagnostic_json text,
  error text,
  created_at text not null,
  finished_at text
);

create table if not exists daily_topics (
  id text primary key,
  day text not null,
  profile_id text not null,
  title text not null,
  summary text not null,
  keywords_json text not null default '[]',
  source_message_ids text not null default '[]',
  updated_at text not null
);

create index if not exists idx_daily_topics_day_profile on daily_topics(day, profile_id);

create table if not exists ai_config (
  id integer primary key check(id = 1),
  provider text not null default '本地DeepSeek',
  api_key text not null default '',
  base_url text not null default 'http://127.0.0.1:11434/v1',
  model text not null default 'deepseek-r1-distill-qwen-7b-q4_k_m',
  user_prompt text not null default '',
  analysis_prompt text not null default '',
  summary_prompt text not null default '',
  analysis_prompt_custom integer,
  summary_prompt_custom integer,
  analysis_batch_size integer not null default 20,
  enabled integer not null default 1,
  test_status text not null default 'untested',
  updated_at text not null
);

insert or ignore into app_meta(key, value, updated_at)
values ('current_day', date('now', 'localtime'), datetime('now'));
