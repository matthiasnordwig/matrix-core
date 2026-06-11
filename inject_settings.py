import re

# Read original
with open("/tmp/pages.tsx.old", "r") as f:
    old_text = f.read()

# Extract form
pattern_form_card = re.compile(r'      <div className="card">\s*<h3>\{editing \? "Endpunkt bearbeiten" : "Endpunkt hinzufügen"\}</h3>.*?</div>\s*</div>\s*<ul className="list">', re.DOTALL)
match = pattern_form_card.search(old_text)
if not match:
    print("Could not find form in old text.")
    exit(1)

form_card = match.group(0)
form_div_match = re.search(r'(<div className="form">.*</div>)\s*</div>\s*<ul className="list">', form_card, re.DOTALL)
form_jsx = form_div_match.group(1)

# Replace the abbrechen button
form_jsx = form_jsx.replace('{editing && <button className="link" onClick={resetForm}>abbrechen</button>}', '<button className="link" onClick={resetForm}>Cancel</button>')

# Read current
with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    current_text = f.read()

# I need to insert `const endpointForm = (\n{form_jsx}\n);\n` right before `return (` in `SettingsTab`.
# Let's find `const modelInOpts = opts.some((o) => o.name === selModel);` which is right before the return in SettingsTab.
injection_point = "  const modelInOpts = opts.some((o) => o.name === selModel);"
if injection_point not in current_text:
    print("Could not find injection point.")
    exit(1)

new_injection = f"  const modelInOpts = opts.some((o) => o.name === selModel);\n\n  const endpointForm = (\n{form_jsx}\n  );"
current_text = current_text.replace(injection_point, new_injection)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(current_text)

print("done")
