"""
Test suite for Source of Truth Consensus with focus on truth determination and source reliability
"""
import os
import sys
import json
import time
import random
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, Any, List, Tuple
import numpy as np

# Add parent directory to path for imports
sys.path.append(str(Path(__file__).parent.parent.parent.parent))

from algorithms.consensus import SourceOfTruthConsensus
from algorithms.test.data.db_config import TestDatabaseConfig

class ConsensusTestMetrics:
    """Track performance metrics for consensus tests"""
    def __init__(self):
        self.start_time = time.time()
        self.metrics = {
            'success': 0,
            'failed': 0,
            'time': 0,
            'accuracy': {
                'truth_determination': [],
                'source_reliability': [],
                'conflict_resolution': [],
                'temporal_consistency': []
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

def generate_test_sources() -> Dict[str, Dict[str, Any]]:
    """Generate test sources with varying reliability levels"""
    source_types = {
        'official': {
            'reliability_range': (0.8, 1.0),
            'examples': ['protocol_docs', 'team_announcement', 'audit_report']
        },
        'community': {
            'reliability_range': (0.4, 0.8),
            'examples': ['forum_post', 'community_review', 'user_feedback']
        },
        'social_media': {
            'reliability_range': (0.2, 0.6),
            'examples': ['twitter_post', 'telegram_message', 'reddit_comment']
        },
        'anonymous': {
            'reliability_range': (0.1, 0.4),
            'examples': ['anonymous_tip', 'unverified_claim', 'rumor']
        }
    }
    
    sources = {}
    for source_type, props in source_types.items():
        for example in props['examples']:
            source_id = f"{source_type}_{example}"
            reliability = random.uniform(*props['reliability_range'])
            
            sources[source_id] = {
                'source_id': source_id,
                'type': source_type,
                'subtype': example,
                'reliability': reliability,
                'verification_status': 'verified' if reliability > 0.7 else 'unverified',
                'historical_accuracy': reliability * random.uniform(0.9, 1.1)
            }
    
    return sources

def generate_test_claims() -> Dict[str, Dict[str, Any]]:
    """Generate test claims with varying complexity and verifiability"""
    return {
        'technical_claim': {
            'claim_id': 'tech_1',
            'text': "Protocol X has implemented zero-knowledge proofs in their latest update",
            'category': 'technical',
            'verifiability': 'high',
            'complexity': 0.8,
            'true_state': True,
            'supporting_evidence': ['github_commit', 'technical_docs', 'audit_confirmation'],
            'contradicting_evidence': ['old_documentation']
        },
        'security_claim': {
            'claim_id': 'security_1',
            'text': "A critical vulnerability has been found in Protocol X's smart contracts",
            'category': 'security',
            'verifiability': 'high',
            'complexity': 0.9,
            'true_state': False,
            'supporting_evidence': ['anonymous_report', 'social_media_panic'],
            'contradicting_evidence': ['security_audit', 'team_statement']
        },
        'market_claim': {
            'claim_id': 'market_1',
            'text': "Protocol X is planning to launch a new token next month",
            'category': 'market',
            'verifiability': 'medium',
            'complexity': 0.6,
            'true_state': None,  # Unknown truth
            'supporting_evidence': ['insider_leak', 'community_speculation'],
            'contradicting_evidence': ['team_denial']
        },
        'governance_claim': {
            'claim_id': 'gov_1',
            'text': "Protocol X's governance token holders have approved proposal Y",
            'category': 'governance',
            'verifiability': 'high',
            'complexity': 0.7,
            'true_state': True,
            'supporting_evidence': ['on_chain_votes', 'governance_forum', 'official_announcement'],
            'contradicting_evidence': []
        }
    }

def test_truth_determination(consensus: SourceOfTruthConsensus,
                           claims: Dict[str, Dict[str, Any]],
                           sources: Dict[str, Dict[str, Any]],
                           metrics: ConsensusTestMetrics) -> None:
    """Test truth determination for various claims"""
    print("\n🔍 Testing Truth Determination:")
    
    for claim_id, claim_data in claims.items():
        start_time = time.time()
        try:
            # Prepare source statements
            source_statements = []
            
            # Add supporting statements
            for evidence in claim_data['supporting_evidence']:
                source_type = 'official' if evidence in ['technical_docs', 'audit_confirmation', 'team_statement'] else 'community'
                source_id = f"{source_type}_{evidence}"
                if source_id in sources:
                    source_statements.append({
                        'source_id': source_id,
                        'reliability': sources[source_id]['reliability'],
                        'statement': 'support',
                        'evidence': evidence
                    })
            
            # Add contradicting statements
            for evidence in claim_data['contradicting_evidence']:
                source_type = 'official' if evidence in ['security_audit', 'team_statement'] else 'community'
                source_id = f"{source_type}_{evidence}"
                if source_id in sources:
                    source_statements.append({
                        'source_id': source_id,
                        'reliability': sources[source_id]['reliability'],
                        'statement': 'contradict',
                        'evidence': evidence
                    })
            
            # Process through consensus mechanism
            result = consensus.process({
                'claim_id': claim_id,
                'claim_text': claim_data['text'],
                'sources': source_statements,
                'metadata': {
                    'category': claim_data['category'],
                    'verifiability': claim_data['verifiability'],
                    'complexity': claim_data['complexity']
                }
            }).results[0]
            
            # Calculate accuracy
            if claim_data['true_state'] is not None:
                truth_score = result.get('truth_score', 0.5)
                accuracy = 1.0 - abs(float(claim_data['true_state']) - truth_score)
            else:
                # For unknown truth, check if uncertainty is reflected
                truth_score = result.get('truth_score', 0.5)
                uncertainty = result.get('uncertainty', 0)
                accuracy = uncertainty if 0.4 <= truth_score <= 0.6 else 0.0
            
            print(f"  ✅ Claim: {claim_id}")
            print(f"    - Truth Score: {truth_score:.2f}")
            print(f"    - Accuracy: {accuracy:.2%}")
            
            metrics.record_success('truth_determination', accuracy, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed truth determination for {claim_id}: {str(e)}")
            metrics.record_failure(f"truth_{claim_id}", time.time() - start_time)

def test_source_reliability(consensus: SourceOfTruthConsensus,
                          sources: Dict[str, Dict[str, Any]],
                          metrics: ConsensusTestMetrics) -> None:
    """Test source reliability assessment"""
    print("\n👥 Testing Source Reliability Assessment:")
    
    # Group sources by type for testing
    source_groups = {}
    for source_id, source_data in sources.items():
        source_type = source_data['type']
        if source_type not in source_groups:
            source_groups[source_type] = []
        source_groups[source_type].append(source_data)
    
    for source_type, source_list in source_groups.items():
        start_time = time.time()
        try:
            # Test reliability assessment for the group
            reliability_scores = []
            for source in source_list:
                result = consensus.process({
                    'source_id': source['source_id'],
                    'reliability_check': True,
                    'source_data': {
                        'type': source['type'],
                        'verification_status': source['verification_status'],
                        'historical_accuracy': source['historical_accuracy']
                    }
                }).results[0]
                
                # Compare assessed reliability with known reliability
                assessed_reliability = result.get('reliability_score', 0)
                actual_reliability = source['reliability']
                accuracy = 1.0 - abs(assessed_reliability - actual_reliability)
                reliability_scores.append(accuracy)
            
            # Calculate average accuracy for the source type
            avg_accuracy = sum(reliability_scores) / len(reliability_scores) if reliability_scores else 0
            
            print(f"  ✅ Source Type: {source_type}")
            print(f"    - Average Accuracy: {avg_accuracy:.2%}")
            
            metrics.record_success('source_reliability', avg_accuracy, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed reliability assessment for {source_type}: {str(e)}")
            metrics.record_failure(f"reliability_{source_type}", time.time() - start_time)

def test_conflict_resolution(consensus: SourceOfTruthConsensus,
                           claims: Dict[str, Dict[str, Any]],
                           sources: Dict[str, Dict[str, Any]],
                           metrics: ConsensusTestMetrics) -> None:
    """Test conflict resolution between contradicting sources"""
    print("\n⚖️ Testing Conflict Resolution:")
    
    # Create conflict scenarios
    conflict_scenarios = [
        {
            'scenario_id': 'conflict_1',
            'claim': claims['security_claim'],
            'supporting_sources': [
                sources.get('anonymous_anonymous_tip'),
                sources.get('social_media_twitter_post')
            ],
            'contradicting_sources': [
                sources.get('official_protocol_docs'),
                sources.get('official_audit_report')
            ],
            'expected_resolution': 'false'  # The claim should be determined as false
        },
        {
            'scenario_id': 'conflict_2',
            'claim': claims['market_claim'],
            'supporting_sources': [
                sources.get('community_forum_post'),
                sources.get('anonymous_unverified_claim')
            ],
            'contradicting_sources': [
                sources.get('official_team_announcement')
            ],
            'expected_resolution': 'uncertain'  # The truth should be uncertain
        }
    ]
    
    for scenario in conflict_scenarios:
        start_time = time.time()
        try:
            # Prepare source statements
            source_statements = []
            
            # Add supporting sources
            for source in scenario['supporting_sources']:
                if source:
                    source_statements.append({
                        'source_id': source['source_id'],
                        'reliability': source['reliability'],
                        'statement': 'support'
                    })
            
            # Add contradicting sources
            for source in scenario['contradicting_sources']:
                if source:
                    source_statements.append({
                        'source_id': source['source_id'],
                        'reliability': source['reliability'],
                        'statement': 'contradict'
                    })
            
            # Process through consensus mechanism
            result = consensus.process({
                'claim_id': scenario['scenario_id'],
                'claim_text': scenario['claim']['text'],
                'sources': source_statements,
                'metadata': {
                    'category': scenario['claim']['category'],
                    'verifiability': scenario['claim']['verifiability'],
                    'complexity': scenario['claim']['complexity']
                }
            }).results[0]
            
            # Evaluate resolution accuracy
            truth_score = result.get('truth_score', 0.5)
            uncertainty = result.get('uncertainty', 0)
            
            if scenario['expected_resolution'] == 'true':
                accuracy = truth_score
            elif scenario['expected_resolution'] == 'false':
                accuracy = 1.0 - truth_score
            else:  # uncertain
                accuracy = uncertainty if 0.4 <= truth_score <= 0.6 else 0.0
            
            print(f"  ✅ Scenario: {scenario['scenario_id']}")
            print(f"    - Resolution Accuracy: {accuracy:.2%}")
            
            metrics.record_success('conflict_resolution', accuracy, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed conflict resolution for {scenario['scenario_id']}: {str(e)}")
            metrics.record_failure(f"conflict_{scenario['scenario_id']}", time.time() - start_time)

def generate_markdown_report(summary: Dict[str, Any]) -> str:
    """Generate a detailed markdown report of the consensus test results"""
    report = [
        "# Source of Truth Consensus Test Results",
        f"\n## Test Overview",
        f"- **Test Date**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        f"- **Total Execution Time**: {summary['total_execution_time']:.2f} seconds",
        f"- **Overall Success Rate**: {summary['success_rate']:.2%}",
        f"- **Average Processing Time**: {summary['average_processing_time']:.3f} seconds",
        "\n## Performance by Category\n"
    ]
    
    # Add accuracy by category
    for category, accuracy in summary['category_accuracy'].items():
        report.extend([
            f"### {category.replace('_', ' ').title()}",
            f"- **Accuracy**: {accuracy:.2%}"
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
            report.append(f"- Improve {category.replace('_', ' ')} (current accuracy: {accuracy:.2%})")
    
    return "\n".join(report)

def main(report_mode: bool = False):
    """Main test runner for consensus system"""
    print("🚀 Starting Source of Truth Consensus Tests")
    
    # Initialize metrics and consensus system
    metrics = ConsensusTestMetrics()
    consensus = SourceOfTruthConsensus(db_config=TestDatabaseConfig())
    
    try:
        # Generate test data
        sources = generate_test_sources()
        claims = generate_test_claims()
        
        # Run tests
        test_truth_determination(consensus, claims, sources, metrics)
        test_source_reliability(consensus, sources, metrics)
        test_conflict_resolution(consensus, claims, sources, metrics)
        
        # Get test summary
        summary = metrics.get_summary()
        
        if report_mode:
            return summary
        
        # Generate and save standalone report
        report = generate_markdown_report(summary)
        report_path = Path(__file__).parent / 'consensus_results.md'
        with open(report_path, 'w') as f:
            f.write(report)
        
        print("\n📊 Test Results Summary:")
        print(f"- Success Rate: {summary['success_rate']:.2%}")
        print(f"- Total Time: {summary['total_execution_time']:.2f} seconds")
        print(f"\nDetailed results written to: {report_path}")
        
    except Exception as e:
        print(f"\n❌ Test suite failed: {str(e)}")
        raise
    
    print("\n✅ Consensus tests completed")

if __name__ == "__main__":
    main() 