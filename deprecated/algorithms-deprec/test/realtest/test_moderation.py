"""
Test suite for Community Moderation System with real-world scenarios
"""
import os
import sys
import json
import time
import random
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, Any, List

# Add parent directory to path for imports
sys.path.append(str(Path(__file__).parent.parent.parent.parent))

from algorithms.community_moderation import CommunityModerationSystem
from algorithms.test.data.db_config import TestDatabaseConfig

class ModerationTestMetrics:
    """Track performance metrics for moderation tests"""
    def __init__(self):
        self.start_time = time.time()
        self.metrics = {
            'success': 0,
            'failed': 0,
            'time': 0,
            'edge_cases': 0,
            'accuracy': {
                'hate_speech': [],
                'spam': [],
                'misinformation': [],
                'coordinated': [],
                'borderline': []
            }
        }
        self.failed_scenarios = []
    
    def record_success(self, category: str, accuracy: float, execution_time: float):
        self.metrics['success'] += 1
        self.metrics['time'] += execution_time
        self.metrics['accuracy'][category].append(accuracy)
    
    def record_failure(self, scenario: str, execution_time: float):
        self.metrics['failed'] += 1
        self.metrics['time'] += execution_time
        self.failed_scenarios.append(scenario)
    
    def get_summary(self) -> Dict[str, Any]:
        total_time = time.time() - self.start_time
        accuracy_summary = {}
        for category, scores in self.metrics['accuracy'].items():
            if scores:
                accuracy_summary[category] = sum(scores) / len(scores)
            else:
                accuracy_summary[category] = 0.0
        
        return {
            'total_execution_time': total_time,
            'success_rate': self.metrics['success'] / (self.metrics['success'] + self.metrics['failed']) if self.metrics['success'] + self.metrics['failed'] > 0 else 0,
            'average_processing_time': self.metrics['time'] / (self.metrics['success'] + self.metrics['failed']) if self.metrics['success'] + self.metrics['failed'] > 0 else 0,
            'category_accuracy': accuracy_summary,
            'failed_scenarios': self.failed_scenarios
        }

def generate_test_cases() -> Dict[str, List[Dict[str, Any]]]:
    """Generate complex test cases for each moderation category"""
    return {
        'hate_speech': [
            {
                'content_id': 'hate_1',
                'text': "You're all idiots for investing in this protocol. [ethnic slur] don't understand technology.",
                'expected_flags': ['hate_speech', 'personal_attack', 'discriminatory'],
                'expected_action': 'remove',
                'context': {'user_reports': 5, 'previous_violations': 2}
            },
            {
                'content_id': 'hate_2',
                'text': "This community would be better if we got rid of all the [demographic] members",
                'expected_flags': ['hate_speech', 'discriminatory', 'community_harm'],
                'expected_action': 'remove',
                'context': {'user_reports': 8, 'previous_violations': 0}
            }
        ],
        'spam': [
            {
                'content_id': 'spam_1',
                'text': "🚀🌙 1000x GEM ALERT! 💎💰 Don't miss out on SCAMCOIN! Limited presale starting now! Join t.me/scam",
                'expected_flags': ['spam', 'excessive_promotion', 'suspicious_links', 'manipulation'],
                'expected_action': 'remove',
                'context': {'user_reports': 3, 'previous_violations': 1}
            },
            {
                'content_id': 'spam_2',
                'text': "Check out my new NFT collection! Only 100 available! DM for whitelist! 🎨🔥 FOMO alert!",
                'expected_flags': ['spam', 'promotional', 'excessive_emojis'],
                'expected_action': 'warn',
                'context': {'user_reports': 2, 'previous_violations': 0}
            }
        ],
        'misinformation': [
            {
                'content_id': 'misinfo_1',
                'text': "BREAKING: Major protocol hacked! All funds stolen! Devs running away with millions! Share this now!",
                'expected_flags': ['misinformation', 'fud', 'unverified_claims'],
                'expected_action': 'remove',
                'context': {'user_reports': 10, 'previous_violations': 0}
            },
            {
                'content_id': 'misinfo_2',
                'text': "Inside source tells me the team is secretly selling tokens. Major dump incoming!",
                'expected_flags': ['potential_misinformation', 'market_manipulation'],
                'expected_action': 'flag',
                'context': {'user_reports': 4, 'previous_violations': 0}
            }
        ],
        'coordinated': [
            {
                'content_id': 'coord_1',
                'text': "This protocol is a scam! Devs are frauds! Sell now!",
                'expected_flags': ['coordinated_fud', 'manipulation'],
                'expected_action': 'remove',
                'context': {
                    'user_reports': 6,
                    'previous_violations': 1,
                    'pattern': {
                        'similar_posts': 15,
                        'time_window': '1h',
                        'coordinated_accounts': True
                    }
                }
            }
        ],
        'borderline': [
            {
                'content_id': 'border_1',
                'text': "The devs are incompetent. This update is garbage. Worst protocol ever.",
                'expected_flags': ['low_quality', 'negative'],
                'expected_action': 'warn',
                'context': {'user_reports': 2, 'previous_violations': 0}
            },
            {
                'content_id': 'border_2',
                'text': "Protocol performance has been terrible. Many users losing money.",
                'expected_flags': ['controversial'],
                'expected_action': 'flag',
                'context': {'user_reports': 3, 'previous_violations': 0}
            }
        ]
    }

def test_moderation_category(moderator: CommunityModerationSystem, 
                           category: str, 
                           test_cases: List[Dict[str, Any]], 
                           metrics: ModerationTestMetrics) -> None:
    """Test moderation system for a specific category of content"""
    print(f"\n🔍 Testing {category} detection:")
    
    for case in test_cases:
        start_time = time.time()
        try:
            # Process content through moderation
            result = moderator.process({
                'content_id': case['content_id'],
                'text': case['text'],
                'context': case['context']
            }).results[0]
            
            # Verify flag detection
            detected_flags = set(result.get('flags', []))
            expected_flags = set(case['expected_flags'])
            flag_accuracy = len(detected_flags.intersection(expected_flags)) / len(expected_flags)
            
            # Verify action
            action_correct = result.get('action') == case['expected_action']
            
            # Calculate overall accuracy
            accuracy = (flag_accuracy + float(action_correct)) / 2
            
            print(f"  ✅ Case {case['content_id']}:")
            print(f"    - Flag Accuracy: {flag_accuracy:.2%}")
            print(f"    - Action Accuracy: {'✓' if action_correct else '✗'}")
            
            metrics.record_success(category, accuracy, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed case {case['content_id']}: {str(e)}")
            metrics.record_failure(f"{category}_{case['content_id']}", time.time() - start_time)

def generate_markdown_report(summary: Dict[str, Any]) -> str:
    """Generate a detailed markdown report of the moderation test results"""
    report = [
        "# Community Moderation System Test Results",
        f"\n## Test Overview",
        f"- **Test Date**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        f"- **Total Execution Time**: {summary['total_execution_time']:.2f} seconds",
        f"- **Overall Success Rate**: {summary['success_rate']:.2%}",
        f"- **Average Processing Time**: {summary['average_processing_time']:.3f} seconds",
        "\n## Category Performance\n"
    ]
    
    # Add accuracy by category
    for category, accuracy in summary['category_accuracy'].items():
        report.extend([
            f"### {category.replace('_', ' ').title()}",
            f"- **Detection Accuracy**: {accuracy:.2%}"
        ])
    
    # Add failed scenarios if any
    if summary['failed_scenarios']:
        report.extend([
            "\n## Failed Scenarios",
            "The following test cases failed:"
        ])
        for scenario in summary['failed_scenarios']:
            report.append(f"- {scenario}")
    
    # Add recommendations based on results
    report.extend([
        "\n## Recommendations",
        "Based on the test results, consider the following improvements:"
    ])
    
    for category, accuracy in summary['category_accuracy'].items():
        if accuracy < 0.8:
            report.append(f"- Improve {category.replace('_', ' ')} detection (current accuracy: {accuracy:.2%})")
    
    return "\n".join(report)

def main(report_mode: bool = False):
    """Main test runner for moderation system"""
    print("🚀 Starting Community Moderation System Tests")
    
    # Initialize metrics and moderation system
    metrics = ModerationTestMetrics()
    moderator = CommunityModerationSystem(db_config=TestDatabaseConfig())
    
    try:
        # Generate test cases
        test_cases = generate_test_cases()
        
        # Run tests for each category
        for category, cases in test_cases.items():
            test_moderation_category(moderator, category, cases, metrics)
        
        # Get test summary
        summary = metrics.get_summary()
        
        if report_mode:
            return summary
        
        # Generate and save standalone report
        report = generate_markdown_report(summary)
        report_path = Path(__file__).parent / 'moderation_results.md'
        with open(report_path, 'w') as f:
            f.write(report)
        
        print("\n📊 Test Results Summary:")
        print(f"- Success Rate: {summary['success_rate']:.2%}")
        print(f"- Total Time: {summary['total_execution_time']:.2f} seconds")
        print(f"\nDetailed results written to: {report_path}")
        
    except Exception as e:
        print(f"\n❌ Test suite failed: {str(e)}")
        raise
    
    print("\n✅ Moderation tests completed")

if __name__ == "__main__":
    main() 