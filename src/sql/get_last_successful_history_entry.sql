SELECT started_at
FROM history
WHERE NOT failed AND ended_at IS NOT NULL
ORDER BY started_at DESC
LIMIT 1;
