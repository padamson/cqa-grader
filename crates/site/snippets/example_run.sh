cqa-grader baseline --graph wine.ttl --benchmark wine-benchmark.ttl --output trace.json
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json
