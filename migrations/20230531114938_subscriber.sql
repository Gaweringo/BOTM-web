CREATE TABLE users (
  spotify_id TEXT NOT NULL,
  PRIMARY KEY(spotify_id),
  active BOOLEAN NOT NULL,
  refresh_token TEXT NOT NULL
);

CREATE TABLE botm_runs (
  id SERIAL NOT NULL,
  PRIMARY KEY(id),
  date DATE NOT NULL
);

CREATE TABLE user_botm_runs (
  spotify_id TEXT NOT NULL REFERENCES users(spotify_id),
  botm_run_id INT NOT NULL REFERENCES botm_runs(id)
);
