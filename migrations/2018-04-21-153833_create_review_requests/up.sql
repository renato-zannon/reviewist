CREATE TABLE review_requests (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,

  project VARCHAR(50) NOT NULL,
  pr_number VARCHAR(6) NOT NULL,
  pr_url VARCHAR(255) NOT NULL,

  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
