import sqlite3

db_path = "/Users/matthias.nordwig/Library/Application Support/com.matthiasnordwig.matrix/matrix.db"
conn = sqlite3.connect(db_path)
cur = conn.cursor()

cur.execute("SELECT id FROM structural_profiles ORDER BY id LIMIT 1")
row = cur.fetchone()
if row:
    profile_id = row[0]
    
    cur.execute("SELECT id FROM structural_patterns WHERE profile_id = ? AND regex = '^(\\(\\d+\\))\\s*(.*)'", (profile_id,))
    if not cur.fetchone():
        cur.execute("""
            INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, sort_order)
            VALUES (?, 'Aufzählungen (Nummern)', 'heading_l1', '^(\\(\\d+\\))\\s*(.*)', 'i', 110, 4)
        """, (profile_id,))
        conn.commit()
        print("Successfully added numbering pattern to database!")
    else:
        print("Pattern already exists!")
else:
    print("No structural profile found in DB.")

conn.close()
