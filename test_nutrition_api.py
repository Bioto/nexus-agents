#!/usr/bin/env python3
"""
Test script for Nutrition API endpoints.
Tests all read-only endpoints for mobile app integration.
"""

import json
import sys
import requests
from typing import Optional, Dict, Any
from urllib.parse import urljoin

class NutritionAPITester:
    def __init__(self, base_url: str = "http://localhost:8080"):
        self.base_url = base_url.rstrip('/')
        self.api_base = f"{self.base_url}/api/nutrition"
        self.session = requests.Session()
        self.results = {
            "passed": [],
            "failed": [],
            "skipped": []
        }
    
    def log(self, message: str, level: str = "INFO"):
        """Print formatted log message"""
        prefix = {
            "INFO": "ℹ️ ",
            "PASS": "✅",
            "FAIL": "❌",
            "SKIP": "⏭️ "
        }.get(level, "  ")
        print(f"{prefix} {message}")
    
    def test_endpoint(
        self,
        name: str,
        method: str,
        path: str,
        expected_status: int = 200,
        data: Optional[Dict[str, Any]] = None,
        params: Optional[Dict[str, Any]] = None,
        skip_if_empty: bool = False
    ) -> Optional[Dict[str, Any]]:
        """Test an API endpoint"""
        url = urljoin(self.api_base + "/", path.lstrip('/'))
        
        try:
            if method.upper() == "GET":
                response = self.session.get(url, params=params, timeout=5)
            elif method.upper() == "POST":
                response = self.session.post(url, json=data, timeout=5)
            else:
                raise ValueError(f"Unsupported method: {method}")
            
            if response.status_code == expected_status:
                if skip_if_empty and len(response.json()) == 0:
                    self.log(f"{name} - No data (skipped)", "SKIP")
                    self.results["skipped"].append(name)
                    return None
                else:
                    self.log(f"{name} - Status {response.status_code}", "PASS")
                    self.results["passed"].append(name)
                    return response.json()
            else:
                self.log(
                    f"{name} - Expected {expected_status}, got {response.status_code}: {response.text[:100]}",
                    "FAIL"
                )
                self.results["failed"].append(name)
                return None
        except requests.exceptions.ConnectionError:
            self.log(f"{name} - Connection error (is server running?)", "FAIL")
            self.results["failed"].append(name)
            return None
        except Exception as e:
            self.log(f"{name} - Error: {str(e)}", "FAIL")
            self.results["failed"].append(name)
            return None
    
    def test_ingredients(self):
        """Test ingredient endpoints"""
        self.log("\n=== Testing Ingredient Endpoints ===", "INFO")
        
        # List ingredients
        ingredients = self.test_endpoint(
            "List Ingredients",
            "GET",
            "/ingredients"
        )
        
        if ingredients and len(ingredients) > 0:
            first_id = ingredients[0]["id"]
            
            # Get single ingredient
            self.test_endpoint(
                "Get Ingredient",
                "GET",
                f"/ingredients/{first_id}"
            )
            
            # Get ingredient nutrition
            self.test_endpoint(
                "Get Ingredient Nutrition",
                "GET",
                f"/ingredients/{first_id}/nutrition"
            )
            
            # Batch get ingredients
            if len(ingredients) >= 2:
                ids = [ing["id"] for ing in ingredients[:2]]
                self.test_endpoint(
                    "Batch Get Ingredients",
                    "POST",
                    "/ingredients/batch",
                    data={"ids": ids}
                )
        else:
            self.log("No ingredients found, skipping individual tests", "SKIP")
    
    def test_recipes(self):
        """Test recipe endpoints"""
        self.log("\n=== Testing Recipe Endpoints ===", "INFO")
        
        # List recipes
        recipes = self.test_endpoint(
            "List Recipes",
            "GET",
            "/recipes"
        )
        
        if recipes and len(recipes) > 0:
            first_id = recipes[0]["id"]
            
            # Get single recipe (with details)
            recipe = self.test_endpoint(
                "Get Recipe with Details",
                "GET",
                f"/recipes/{first_id}"
            )
            
            # Calculate recipe nutrition
            self.test_endpoint(
                "Calculate Recipe Nutrition",
                "GET",
                f"/recipes/{first_id}/nutrition"
            )
            
            # Batch get recipes
            if len(recipes) >= 2:
                ids = [r["id"] for r in recipes[:2]]
                self.test_endpoint(
                    "Batch Get Recipes",
                    "POST",
                    "/recipes/batch",
                    data={"ids": ids}
                )
            
            # Get favorited by
            self.test_endpoint(
                "Get Recipe Favorited By",
                "GET",
                f"/recipes/{first_id}/favorited-by",
                skip_if_empty=True
            )
        else:
            self.log("No recipes found, skipping individual tests", "SKIP")
    
    def test_meal_plans(self):
        """Test meal plan endpoints"""
        self.log("\n=== Testing Meal Plan Endpoints ===", "INFO")
        
        # List meal plans
        meal_plans = self.test_endpoint(
            "List Meal Plans",
            "GET",
            "/meal-plans"
        )
        
        if meal_plans and len(meal_plans) > 0:
            first_id = meal_plans[0]["id"]
            
            # Get single meal plan
            self.test_endpoint(
                "Get Meal Plan",
                "GET",
                f"/meal-plans/{first_id}"
            )
            
            # Get meal plan with entries
            self.test_endpoint(
                "Get Meal Plan with Entries",
                "GET",
                f"/meal-plans/{first_id}/entries"
            )
            
            # Calculate meal plan nutrition
            self.test_endpoint(
                "Calculate Meal Plan Nutrition",
                "GET",
                f"/meal-plans/{first_id}/nutrition"
            )
            
            # Get meal plan prep analysis
            self.test_endpoint(
                "Get Meal Plan Prep Analysis",
                "GET",
                f"/meal-plans/{first_id}/prep-analysis"
            )
            
            # Batch get meal plans
            if len(meal_plans) >= 2:
                ids = [mp["id"] for mp in meal_plans[:2]]
                self.test_endpoint(
                    "Batch Get Meal Plans",
                    "POST",
                    "/meal-plans/batch",
                    data={"ids": ids}
                )
            
            # Test filters
            self.test_endpoint(
                "List Meal Plans (with search)",
                "GET",
                "/meal-plans",
                params={"search": meal_plans[0]["name"][:5] if meal_plans[0]["name"] else ""}
            )
        else:
            self.log("No meal plans found, skipping individual tests", "SKIP")
    
    def test_family_members(self):
        """Test family member endpoints"""
        self.log("\n=== Testing Family Member Endpoints ===", "INFO")
        
        # List family members
        family_members = self.test_endpoint(
            "List Family Members",
            "GET",
            "/family-members"
        )
        
        if family_members and len(family_members) > 0:
            first_id = family_members[0]["id"]
            
            # Get single family member
            self.test_endpoint(
                "Get Family Member",
                "GET",
                f"/family-members/{first_id}"
            )
            
            # Get family member with allergies
            self.test_endpoint(
                "Get Family Member with Allergies",
                "GET",
                f"/family-members/{first_id}/allergies"
            )
            
            # Get family member favorites
            self.test_endpoint(
                "Get Family Member Favorites",
                "GET",
                f"/family-members/{first_id}/favorites",
                skip_if_empty=True
            )
            
            # Batch get family members
            if len(family_members) >= 2:
                ids = [fm["id"] for fm in family_members[:2]]
                self.test_endpoint(
                    "Batch Get Family Members",
                    "POST",
                    "/family-members/batch",
                    data={"ids": ids}
                )
            
            # Test search
            self.test_endpoint(
                "List Family Members (with search)",
                "GET",
                "/family-members",
                params={"search": family_members[0]["name"][:3] if family_members[0]["name"] else ""}
            )
        else:
            self.log("No family members found, skipping individual tests", "SKIP")
    
    def test_health_check(self):
        """Test if server is running"""
        self.log("\n=== Health Check ===", "INFO")
        try:
            response = self.session.get(self.base_url, timeout=2)
            self.log(f"Server responded (status: {response.status_code})", "PASS")
            return True
        except requests.exceptions.ConnectionError:
            self.log("Cannot connect to server. Is it running?", "FAIL")
            self.log(f"Expected server at: {self.base_url}", "INFO")
            return False
        except Exception as e:
            self.log(f"Health check error: {str(e)}", "FAIL")
            return False
    
    def run_all_tests(self):
        """Run all test suites"""
        self.log("=" * 60, "INFO")
        self.log("Nutrition API Test Suite", "INFO")
        self.log("=" * 60, "INFO")
        
        if not self.test_health_check():
            self.log("\n⚠️  Server not available. Please start the API server first:", "INFO")
            self.log("   x-toolbox nutrition server --host 0.0.0.0 --port 8080", "INFO")
            return
        
        self.test_ingredients()
        self.test_recipes()
        self.test_meal_plans()
        self.test_family_members()
        
        # Print summary
        self.log("\n" + "=" * 60, "INFO")
        self.log("Test Summary", "INFO")
        self.log("=" * 60, "INFO")
        print(f"✅ Passed: {len(self.results['passed'])}")
        print(f"❌ Failed: {len(self.results['failed'])}")
        print(f"⏭️  Skipped: {len(self.results['skipped'])}")
        
        if self.results['failed']:
            self.log("\nFailed tests:", "INFO")
            for test in self.results['failed']:
                self.log(f"  - {test}", "FAIL")
        
        # Exit with error code if any tests failed
        if self.results['failed']:
            sys.exit(1)
        else:
            sys.exit(0)


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description="Test Nutrition API endpoints")
    parser.add_argument(
        "--url",
        default="http://localhost:8080",
        help="Base URL of the API server (default: http://localhost:8080)"
    )
    
    args = parser.parse_args()
    
    tester = NutritionAPITester(base_url=args.url)
    tester.run_all_tests()


if __name__ == "__main__":
    main()

