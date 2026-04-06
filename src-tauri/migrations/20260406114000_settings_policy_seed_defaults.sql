-- Seed defaults added after the initial schema without mutating historical migrations.

INSERT OR IGNORE INTO settings (key, value_json) VALUES
    ('general.minimize_to_tray', 'true');

INSERT OR IGNORE INTO policies (key, value_json) VALUES
    ('deny_list', '[{"id":"default-deny-rm-root","tool":"shell","pattern":"rm -rf /"},{"id":"default-deny-rm-literal-star","tool":"shell","pattern":"rm -rf \\\\*"}]');

UPDATE policies
SET value_json = '[{"id":"default-deny-rm-root","tool":"shell","pattern":"rm -rf /"},{"id":"default-deny-rm-literal-star","tool":"shell","pattern":"rm -rf \\\\*"}]',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE key = 'deny_list'
  AND value_json = '[]';
