# An EXIT trap set inside a substitution fires there, into the capture.
x=$(trap 'echo E' EXIT; echo body)
echo "[$x]"
