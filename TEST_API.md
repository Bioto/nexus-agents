# Nutrition API Test Scripts

Test scripts to verify all the read-only API endpoints for the mobile app.

## Prerequisites

1. **Start the API server:**
   ```bash
   x-toolbox nutrition server --host 0.0.0.0 --port 8080
   ```

2. **Ensure the database is running:**
   ```bash
   make nutrition-db-start
   # or
   x-toolbox nutrition db start
   ```

## Python Test Script (Recommended)

Comprehensive test script with detailed output and error handling.

### Requirements
```bash
pip install requests
```

### Usage
```bash
# Test with default URL (http://localhost:8080)
./test_nutrition_api.py

# Test with custom URL
./test_nutrition_api.py --url http://localhost:3000
```

### Features
- ✅ Tests all read-only endpoints
- ✅ Handles empty data gracefully
- ✅ Color-coded output
- ✅ Summary report
- ✅ Exit codes for CI/CD integration

## Shell Test Script (Quick Check)

Simple bash script using curl for quick verification.

### Requirements
- `curl`
- `jq` (for JSON parsing)

### Usage
```bash
# Test with default URL (http://localhost:8080)
./test_nutrition_api.sh

# Test with custom URL
./test_nutrition_api.sh http://localhost:3000
```

## Test Coverage

The scripts test all read-only endpoints:

### Ingredients
- ✅ List ingredients
- ✅ Get ingredient by ID
- ✅ Get ingredient nutrition
- ✅ Batch get ingredients

### Recipes
- ✅ List recipes
- ✅ Get recipe with details
- ✅ Calculate recipe nutrition
- ✅ Batch get recipes
- ✅ Get recipe favorited by

### Meal Plans
- ✅ List meal plans
- ✅ Get meal plan
- ✅ Get meal plan with entries
- ✅ Calculate meal plan nutrition
- ✅ Get meal plan prep analysis
- ✅ Batch get meal plans
- ✅ Filter meal plans (search, dates, template)

### Family Members
- ✅ List family members
- ✅ Get family member
- ✅ Get family member with allergies
- ✅ Get family member favorites
- ✅ Batch get family members
- ✅ Search family members

## Example Output

```
============================================================
Nutrition API Test Suite
============================================================

=== Health Check ===
✅ Server responded (status: 200)

=== Testing Ingredient Endpoints ===
✅ List Ingredients - Status 200
✅ Get Ingredient - Status 200
✅ Get Ingredient Nutrition - Status 200
✅ Batch Get Ingredients - Status 200

=== Testing Recipe Endpoints ===
✅ List Recipes - Status 200
✅ Get Recipe with Details - Status 200
✅ Calculate Recipe Nutrition - Status 200
...

============================================================
Test Summary
============================================================
✅ Passed: 25
❌ Failed: 0
⏭️  Skipped: 2
```

## Troubleshooting

### Connection Error
If you see connection errors, make sure:
1. The API server is running
2. The port matches (default: 8080)
3. The host is accessible (use `0.0.0.0` to allow external connections)

### Empty Data
Some tests will be skipped if there's no data in the database. This is expected and normal.

### JSON Parsing Errors (Shell Script)
If `jq` is not installed:
```bash
# Ubuntu/Debian
sudo apt-get install jq

# macOS
brew install jq
```

