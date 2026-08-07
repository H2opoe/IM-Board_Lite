create table if not exists diagnostic_error_events (
  id text primary key,
  source text not null check(source in ('cli', 'bridge', 'ai', 'local_ai_runtime', 'sync')),
  category text not null,
  severity text not null check(severity in ('info', 'warning', 'error')),
  profile_id text,
  platform text,
  operation text not null,
  user_message text not null,
  raw_detail_json text not null default '{}',
  context_json text not null default '{}',
  created_at text not null
);

create index if not exists idx_diagnostic_error_events_created
on diagnostic_error_events(created_at desc);

create index if not exists idx_ai_analysis_runs_created
on ai_analysis_runs(created_at desc);
