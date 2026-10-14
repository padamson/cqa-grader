# exit 0: the trace met the requirement; 1: it did not; 2: unusable input
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json \
  --require no-wrong-answers
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json \
  --format json --report report.json
