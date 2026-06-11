with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Let's count the number of <div and </div inside endpointForm
start = text.find('const endpointForm = (')
end = text.find('    </>\n  );\n', start)
form_str = text[start:end]

divs = form_str.count('<div')
enddivs = form_str.count('</div')
print(f"<div: {divs}, </div: {enddivs}")

if divs > enddivs:
    text = text[:end] + '</div>\n' * (divs - enddivs) + text[end:]
    with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
        f.write(text)
    print("Fixed unbalanced divs")

