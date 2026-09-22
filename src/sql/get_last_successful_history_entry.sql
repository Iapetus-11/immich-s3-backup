SELECT id, started_at, ended_at, failed FROM history WHERE NOT failed ORDER BY started_at DESC LIMIT 1;
