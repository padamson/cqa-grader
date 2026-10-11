# exit 0: every question passed; 1: at least one failed; 2: unusable input
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json \
  --format json --report report.json
