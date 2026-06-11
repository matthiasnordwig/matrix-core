import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Replace `const endpointForm = (\n          <label>1 · Provider` 
# with `const endpointForm = (\n        <div className="form">\n          <label>1 · Provider`

text = text.replace('  const endpointForm = (\n          <label>1 · Provider', '  const endpointForm = (\n        <div className="form">\n          <label>1 · Provider')

# Replace `                {editing && <button className="link" onClick={resetForm}>Cancel</button>}\n              </div>\n            </>\n          )}\n  );`
# with `... </div>\n            </>\n          )}\n        </div>\n  );`

pattern_end = re.compile(r'                \{editing && <button className="link" onClick=\{resetForm\}>Cancel</button>\}\n              </div>\n            </>\n          \)\}\n  \);', re.DOTALL)
text = pattern_end.sub('                {editing && <button className="link" onClick={resetForm}>Cancel</button>}\n              </div>\n            </>\n          )}\n        </div>\n  );', text)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")
