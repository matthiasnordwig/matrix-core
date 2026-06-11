with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

start = text.find('const endpointForm = (')
end = text.find('    </>\n  );\n', start) + len('    </>\n  );\n')
form_str = text[start:end]

print("--- form string ---")
print(form_str)
print("--- end ---")
