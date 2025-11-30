#!/bin/bash
# Quick test script for Nutrition API endpoints using curl

API_URL="${1:-http://localhost:8080}/api/nutrition"

echo "=========================================="
echo "Testing Nutrition API"
echo "Base URL: $API_URL"
echo "=========================================="
echo

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

test_endpoint() {
    local name="$1"
    local method="$2"
    local path="$3"
    local expected_status="${4:-200}"
    
    local url="$API_URL/$path"
    local status_code
    
    if [ "$method" = "GET" ]; then
        status_code=$(curl -s -o /dev/null -w "%{http_code}" "$url")
    elif [ "$method" = "POST" ]; then
        status_code=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
            -H "Content-Type: application/json" \
            -d "$5" \
            "$url")
    else
        echo -e "${RED}❌ $name - Unsupported method: $method${NC}"
        return 1
    fi
    
    if [ "$status_code" = "$expected_status" ]; then
        echo -e "${GREEN}✅ $name${NC} (Status: $status_code)"
        return 0
    else
        echo -e "${RED}❌ $name${NC} (Expected: $expected_status, Got: $status_code)"
        return 1
    fi
}

# Test health
echo "=== Health Check ==="
if curl -s -f "$API_URL/../" > /dev/null 2>&1; then
    echo -e "${GREEN}✅ Server is running${NC}"
else
    echo -e "${RED}❌ Cannot connect to server${NC}"
    echo "Please start the server with: x-toolbox nutrition server --host 0.0.0.0 --port 8080"
    exit 1
fi
echo

# Test ingredients
echo "=== Ingredient Endpoints ==="
test_endpoint "List Ingredients" "GET" "ingredients"
INGREDIENT_ID=$(curl -s "$API_URL/ingredients" | jq -r '.[0].id // empty' 2>/dev/null)
if [ -n "$INGREDIENT_ID" ] && [ "$INGREDIENT_ID" != "null" ]; then
    test_endpoint "Get Ingredient" "GET" "ingredients/$INGREDIENT_ID"
    test_endpoint "Get Ingredient Nutrition" "GET" "ingredients/$INGREDIENT_ID/nutrition"
    test_endpoint "Batch Get Ingredients" "POST" "ingredients/batch" 200 "{\"ids\":[\"$INGREDIENT_ID\"]}"
else
    echo -e "${YELLOW}⏭️  No ingredients found, skipping individual tests${NC}"
fi
echo

# Test recipes
echo "=== Recipe Endpoints ==="
test_endpoint "List Recipes" "GET" "recipes"
RECIPE_ID=$(curl -s "$API_URL/recipes" | jq -r '.[0].id // empty' 2>/dev/null)
if [ -n "$RECIPE_ID" ] && [ "$RECIPE_ID" != "null" ]; then
    test_endpoint "Get Recipe" "GET" "recipes/$RECIPE_ID"
    test_endpoint "Calculate Recipe Nutrition" "GET" "recipes/$RECIPE_ID/nutrition"
    test_endpoint "Batch Get Recipes" "POST" "recipes/batch" 200 "{\"ids\":[\"$RECIPE_ID\"]}"
    test_endpoint "Get Recipe Favorited By" "GET" "recipes/$RECIPE_ID/favorited-by"
else
    echo -e "${YELLOW}⏭️  No recipes found, skipping individual tests${NC}"
fi
echo

# Test meal plans
echo "=== Meal Plan Endpoints ==="
test_endpoint "List Meal Plans" "GET" "meal-plans"
MEAL_PLAN_ID=$(curl -s "$API_URL/meal-plans" | jq -r '.[0].id // empty' 2>/dev/null)
if [ -n "$MEAL_PLAN_ID" ] && [ "$MEAL_PLAN_ID" != "null" ]; then
    test_endpoint "Get Meal Plan" "GET" "meal-plans/$MEAL_PLAN_ID"
    test_endpoint "Get Meal Plan with Entries" "GET" "meal-plans/$MEAL_PLAN_ID/entries"
    test_endpoint "Calculate Meal Plan Nutrition" "GET" "meal-plans/$MEAL_PLAN_ID/nutrition"
    test_endpoint "Get Meal Plan Prep Analysis" "GET" "meal-plans/$MEAL_PLAN_ID/prep-analysis"
    test_endpoint "Batch Get Meal Plans" "POST" "meal-plans/batch" 200 "{\"ids\":[\"$MEAL_PLAN_ID\"]}"
else
    echo -e "${YELLOW}⏭️  No meal plans found, skipping individual tests${NC}"
fi
echo

# Test family members
echo "=== Family Member Endpoints ==="
test_endpoint "List Family Members" "GET" "family-members"
FAMILY_MEMBER_ID=$(curl -s "$API_URL/family-members" | jq -r '.[0].id // empty' 2>/dev/null)
if [ -n "$FAMILY_MEMBER_ID" ] && [ "$FAMILY_MEMBER_ID" != "null" ]; then
    test_endpoint "Get Family Member" "GET" "family-members/$FAMILY_MEMBER_ID"
    test_endpoint "Get Family Member with Allergies" "GET" "family-members/$FAMILY_MEMBER_ID/allergies"
    test_endpoint "Get Family Member Favorites" "GET" "family-members/$FAMILY_MEMBER_ID/favorites"
    test_endpoint "Batch Get Family Members" "POST" "family-members/batch" 200 "{\"ids\":[\"$FAMILY_MEMBER_ID\"]}"
else
    echo -e "${YELLOW}⏭️  No family members found, skipping individual tests${NC}"
fi
echo

echo "=========================================="
echo "Testing complete!"
echo "=========================================="

