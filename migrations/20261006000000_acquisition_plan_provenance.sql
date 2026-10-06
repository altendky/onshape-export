CREATE TABLE acquisition_plan_provenance (
    provenance_schema_version INTEGER NOT NULL CHECK (provenance_schema_version = 1),
    plan_identity TEXT NOT NULL,
    provenance_identity TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (provenance_schema_version, plan_identity)
);
