#!/bin/bash
set -euo pipefail

# Compare benchmark results from two files and detect regressions
# Usage: compare-benchmarks.sh <pr-output.txt> <base-output.txt> <threshold>

PR_FILE="${1:-pr-output.txt}"
BASE_FILE="${2:-base-output.txt}"
THRESHOLD="${3:-20}"  # Default 20% regression threshold

echo "## Benchmark Comparison"
echo ""
echo "Comparing PR branch against base branch"
echo "Regression threshold: ${THRESHOLD}%"
echo ""

# Parse bencher output format: "test benchmark_name ... bench: 123 ns/iter (+/- 45)"
parse_bench() {
    local file=$1
    grep "bench:" "$file" | awk '{
        # Extract benchmark name (word after "test")
        for (i=1; i<=NF; i++) {
            if ($i == "test") {
                name = $(i+1)
            }
            if ($i == "bench:") {
                time = $(i+1)
                unit = $(i+2)
                gsub(/,/, "", time)  # Remove commas from numbers
                print name, time, unit
            }
        }
    }'
}

# Calculate percentage change
calc_change() {
    local old=$1
    local new=$2
    awk -v old="$old" -v new="$new" 'BEGIN {
        if (old == 0) print "N/A"
        else printf "%.2f", ((new - old) / old) * 100
    }'
}

# Parse both files
pr_results=$(parse_bench "$PR_FILE")
base_results=$(parse_bench "$BASE_FILE")

has_regression=false
regression_details=""

echo "| Benchmark | Base | PR | Change | Status |"
echo "|-----------|------|----|----|--------|"

while IFS= read -r base_line; do
    bench_name=$(echo "$base_line" | awk '{print $1}')
    base_time=$(echo "$base_line" | awk '{print $2}')
    base_unit=$(echo "$base_line" | awk '{print $3}')

    pr_line=$(echo "$pr_results" | grep "^$bench_name " || echo "")

    if [ -z "$pr_line" ]; then
        echo "| $bench_name | $base_time $base_unit | Missing | N/A | ⚠️ Missing |"
        continue
    fi

    pr_time=$(echo "$pr_line" | awk '{print $2}')
    pr_unit=$(echo "$pr_line" | awk '{print $3}')

    if [ "$base_unit" != "$pr_unit" ]; then
        echo "| $bench_name | $base_time $base_unit | $pr_time $pr_unit | N/A | ⚠️ Unit changed |"
        continue
    fi

    change=$(calc_change "$base_time" "$pr_time")

    if [ "$change" = "N/A" ]; then
        status="⚠️ Cannot compare"
    else
        # Check if regression (positive change means slower)
        is_regression=$(awk -v change="$change" -v threshold="$THRESHOLD" 'BEGIN {
            if (change > threshold) print "yes"
            else print "no"
        }')

        if [ "$is_regression" = "yes" ]; then
            status="❌ Regression"
            has_regression=true
            regression_details="${regression_details}**$bench_name**: +${change}% (${base_time} → ${pr_time} ${pr_unit})\n"
        elif awk -v change="$change" 'BEGIN { exit !(change < -10) }'; then
            status="✅ Improvement"
        else
            status="✔️ OK"
        fi
    fi

    # Format change with sign
    if [ "$change" != "N/A" ]; then
        change_str=$(awk -v c="$change" 'BEGIN {
            if (c > 0) printf "+%.1f%%", c
            else printf "%.1f%%", c
        }')
    else
        change_str="N/A"
    fi

    echo "| $bench_name | $base_time $base_unit | $pr_time $pr_unit | $change_str | $status |"
done <<< "$base_results"

echo ""
echo "---"
echo ""

if [ "$has_regression" = true ]; then
    echo "### ⚠️ Performance Regressions Detected"
    echo ""
    echo "The following benchmarks show regressions exceeding ${THRESHOLD}%:"
    echo ""
    echo -e "$regression_details"
    echo ""
    echo "Please investigate these regressions before merging."
    exit 1
else
    echo "### ✅ No significant regressions detected"
    echo ""
    echo "All benchmarks are within acceptable limits."
fi

echo ""
echo "*Note: Benchmark results on CI can be noisy due to virtualization. Small variations (<${THRESHOLD}%) are normal.*"
