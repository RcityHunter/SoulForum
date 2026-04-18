-- SoulForum points v1: Karma + Merit only.
-- Event-sourced enough for audit/rebuild, but still minimal for current backend.
-- Intentionally excludes Trust from v1.

DEFINE TABLE points_events SCHEMALESS;
DEFINE INDEX idx_points_events_target_created ON TABLE points_events COLUMNS target_member_id, created_at;
DEFINE INDEX idx_points_events_actor_created  ON TABLE points_events COLUMNS actor_member_id, created_at;
DEFINE INDEX idx_points_events_reference      ON TABLE points_events COLUMNS reference_type, reference_id;
DEFINE INDEX idx_points_events_idempotency    ON TABLE points_events COLUMNS idempotency_key UNIQUE;

DEFINE TABLE user_points SCHEMALESS;
DEFINE INDEX idx_user_points_member ON TABLE user_points COLUMNS member_id UNIQUE;
DEFINE INDEX idx_user_points_karma  ON TABLE user_points COLUMNS karma, updated_at;
DEFINE INDEX idx_user_points_merit  ON TABLE user_points COLUMNS merit, updated_at;
