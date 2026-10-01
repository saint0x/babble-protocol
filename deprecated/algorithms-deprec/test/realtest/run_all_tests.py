"""
Main test runner for all algorithms
"""
import os
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.append(str(Path(__file__).parent.parent.parent.parent))

from algorithms.test.realtest.test_moderation import main as run_moderation_tests
from algorithms.test.realtest.test_recommendations import main as run_recommendation_tests
from algorithms.test.realtest.test_consensus import main as run_consensus_tests
from algorithms.test.realtest.report_generator import UnifiedReport, save_unified_report

def main():
    """Run all algorithm tests and generate unified report"""
    print("🚀 Starting Babel Protocol Algorithm Test Suite")
    
    # Initialize unified report
    report = UnifiedReport()
    
    try:
        # Run moderation tests
        print("\n📋 Running Moderation Tests...")
        moderation_metrics = run_moderation_tests(report_mode=True)
        report.add_section("🛡️ Community Moderation System", moderation_metrics)
        
        # Run recommendation tests
        print("\n📋 Running Recommendation Tests...")
        recommendation_metrics = run_recommendation_tests(report_mode=True)
        report.add_section("🎯 Content Recommendation System", recommendation_metrics)
        
        # Run consensus tests
        print("\n📋 Running Consensus Tests...")
        consensus_metrics = run_consensus_tests(report_mode=True)
        report.add_section("🤝 Source of Truth Consensus", consensus_metrics)
        
        # Generate and save unified report
        print("\n📝 Generating Unified Test Report...")
        unified_report = report.generate()
        report_path = save_unified_report(unified_report)
        
        print(f"\n✅ All tests completed! Unified report saved to: {report_path}")
        
    except Exception as e:
        print(f"\n❌ Test suite failed: {str(e)}")
        raise

if __name__ == "__main__":
    main() 