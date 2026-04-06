CREATE TABLE IF NOT EXISTS test_run_script (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL
);

INSERT INTO test_run_script (name) VALUES ('hello'), ('world');
