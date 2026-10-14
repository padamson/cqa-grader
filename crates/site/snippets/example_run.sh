cqa-grader baseline --graph wine.ttl --dataset worked-example --dataset-version 0.2.1 \
  --benchmark wine-benchmark.ttl --output trace.json
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json --require no-wrong-answers
