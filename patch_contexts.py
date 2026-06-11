import re

with open("/Users/matthias.nordwig/matrix/core/src/db/contexts.rs", "r") as f:
    text = f.read()

# Row decoding
text = text.replace('embedding_dim: row.get("embedding_dim")?,', 'embedding_dim: row.get("embedding_dim")?,\n        chunk_endpoint_id: row.get("chunk_endpoint_id")?,')

# create_context INSERT
old_insert_sql = """            "INSERT INTO contexts
                (name, description, chunking_strategy, chunking_profile_id, structural_profile_id, embedding_model_id, embedding_dim)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)","""
new_insert_sql = """            "INSERT INTO contexts
                (name, description, chunking_strategy, chunking_profile_id, structural_profile_id, embedding_model_id, embedding_dim, chunk_endpoint_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)","""
text = text.replace(old_insert_sql, new_insert_sql)

old_insert_params = """                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
            ],"""
new_insert_params = """                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
                c.chunk_endpoint_id,
            ],"""
text = text.replace(old_insert_params, new_insert_params)

# update_context UPDATE
old_update_sql = """            "UPDATE contexts SET
                name = ?2, description = ?3, chunking_strategy = ?4, chunking_profile_id = ?5,
                structural_profile_id = ?6, embedding_model_id = ?7, embedding_dim = ?8, updated_at = unixepoch()
             WHERE id = ?1","""
new_update_sql = """            "UPDATE contexts SET
                name = ?2, description = ?3, chunking_strategy = ?4, chunking_profile_id = ?5,
                structural_profile_id = ?6, embedding_model_id = ?7, embedding_dim = ?8, chunk_endpoint_id = ?9, updated_at = unixepoch()
             WHERE id = ?1","""
text = text.replace(old_update_sql, new_update_sql)

old_update_params = """                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
            ],"""
new_update_params = """                c.chunking_strategy,
                c.chunking_profile_id,
                c.structural_profile_id,
                c.embedding_model_id,
                c.embedding_dim,
                c.chunk_endpoint_id,
            ],"""
text = text.replace(old_update_params, new_update_params)

with open("/Users/matthias.nordwig/matrix/core/src/db/contexts.rs", "w") as f:
    f.write(text)

print("contexts.rs patched")
