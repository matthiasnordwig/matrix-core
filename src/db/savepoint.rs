//! Nestable atomic sections: `SAVEPOINT` starts a transaction when none is open
//! and nests inside one otherwise (unlike `BEGIN`, which fails inside an open
//! transaction). Writers that may run inside a larger replacement — e.g.
//! `insert_chunks_in_order` while a document's chunks are being replaced
//! (`ontology/evidence_carry_db.rs`) — use [`Database::in_savepoint`].
use super::{CoreError, Database};

impl Database {
    /// Run `f` atomically under the savepoint `name` (an SQL identifier): its
    /// changes are released on `Ok` and rolled back on `Err`. The error type
    /// is the caller's, so app code can run its own steps inside.
    pub fn in_savepoint<T, E: From<CoreError>>(
        &self,
        name: &str,
        f: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        self.conn.execute_batch(&format!("SAVEPOINT {name}")).map_err(|e| E::from(e.into()))?;
        match f() {
            Ok(v) => {
                self.conn.execute_batch(&format!("RELEASE {name}")).map_err(|e| E::from(e.into()))?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
                Err(e)
            }
        }
    }
}
