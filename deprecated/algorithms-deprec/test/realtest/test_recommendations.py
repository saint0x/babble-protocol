"""
Test suite for Content Recommendation System with focus on similarity and personalization
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

from algorithms.recommendation import ContentRecommendationSystem
from algorithms.test.data.db_config import TestDatabaseConfig

class RecommendationTestMetrics:
    """Track performance metrics for recommendation tests"""
    def __init__(self):
        self.start_time = time.time()
        self.metrics = {
            'success': 0,
            'failed': 0,
            'time': 0,
            'accuracy': {
                'content_similarity': [],
                'user_preferences': [],
                'diversity': [],
                'temporal_relevance': []
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

def generate_test_content() -> Dict[str, Dict[str, Any]]:
    """Generate diverse test content with clear relationships and similarities"""
    content_categories = {
        'technical': {
            'topics': ['blockchain', 'cryptography', 'smart_contracts'],
            'complexity': (0.7, 1.0),
            'length': (1000, 5000),
            'authenticity': (0.8, 1.0)  # High authenticity for technical content
        },
        'educational': {
            'topics': ['tutorials', 'guides', 'explanations'],
            'complexity': (0.3, 0.7),
            'length': (500, 2000),
            'authenticity': (0.7, 0.9)  # Good authenticity for educational content
        },
        'news': {
            'topics': ['updates', 'announcements', 'events'],
            'complexity': (0.4, 0.8),
            'length': (300, 1500),
            'authenticity': (0.6, 0.8)  # Moderate authenticity for news
        },
        'market': {
            'topics': ['trading', 'analysis', 'price_action'],
            'complexity': (0.5, 0.9),
            'length': (800, 3000),
            'authenticity': (0.5, 0.7)  # Lower authenticity for market analysis
        }
    }
    
    content = {}
    for category, props in content_categories.items():
        for i in range(5):  # 5 pieces of content per category
            content_id = f"{category}_{i}"
            complexity = random.uniform(*props['complexity'])
            length = random.randint(*props['length'])
            authenticity = random.uniform(*props['authenticity'])
            
            content[content_id] = {
                'content_id': content_id,
                'title': f"Test {category.title()} Content {i}",
                'text': f"Sample {category} content with {length} characters...",
                'topics': random.sample(props['topics'], k=2),
                'complexity': complexity,
                'length': length,
                'authenticity': authenticity,
                'timestamp': datetime.now() - timedelta(days=random.randint(0, 30)),
                'engagement_metrics': {
                    'views': random.randint(100, 10000),
                    'likes': random.randint(10, 1000),
                    'shares': random.randint(0, 200)
                }
            }
    
    return content

def generate_test_users() -> Dict[str, Dict[str, Any]]:
    """Generate test users with different preferences and behaviors"""
    user_personas = {
        'technical_expert': {
            'interests': ['blockchain', 'cryptography', 'smart_contracts'],
            'expertise_level': 'expert',
            'preferred_complexity': 0.8,
            'preferred_length': 'long',
            'engagement_pattern': 'deep_dive'
        },
        'casual_learner': {
            'interests': ['tutorials', 'guides', 'basics'],
            'expertise_level': 'beginner',
            'preferred_complexity': 0.4,
            'preferred_length': 'medium',
            'engagement_pattern': 'casual'
        },
        'trader': {
            'interests': ['trading', 'market_analysis', 'price_action'],
            'expertise_level': 'intermediate',
            'preferred_complexity': 0.6,
            'preferred_length': 'medium',
            'engagement_pattern': 'frequent'
        },
        'researcher': {
            'interests': ['technical', 'analysis', 'cryptography'],
            'expertise_level': 'expert',
            'preferred_complexity': 0.9,
            'preferred_length': 'very_long',
            'engagement_pattern': 'deep_dive'
        }
    }
    
    users = {}
    for persona, props in user_personas.items():
        user_id = f"user_{persona}"
        users[user_id] = {
            'user_id': user_id,
            'profile': props,
            'history': {
                'viewed_content': [],
                'liked_content': [],
                'shared_content': []
            }
        }
    
    return users

def test_content_similarity(recommender: ContentRecommendationSystem, 
                          content: Dict[str, Dict[str, Any]], 
                          metrics: RecommendationTestMetrics) -> None:
    """Test content similarity recommendations"""
    print("\n🔍 Testing Content Similarity:")
    
    # Group similar content
    similar_pairs = [
        ('technical_0', 'technical_1'),  # Same category
        ('educational_0', 'educational_1'),  # Same category
        ('technical_0', 'educational_0'),  # Different but related
        ('market_0', 'news_0')  # Different but potentially related
    ]
    
    for content_id1, content_id2 in similar_pairs:
        start_time = time.time()
        try:
            # Get recommendations based on first content
            recommendations = recommender.process({
                'content_id': content_id1,
                'mode': 'similar_content'
            }).results
            
            # Check if similar content is recommended
            rec_ids = [r['content_id'] for r in recommendations]
            similarity_score = 1.0 if content_id2 in rec_ids else 0.0
            
            print(f"  ✅ Testing similarity: {content_id1} -> {content_id2}")
            print(f"    - Found in recommendations: {'✓' if similarity_score == 1.0 else '✗'}")
            
            metrics.record_success('content_similarity', similarity_score, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed similarity test for {content_id1}: {str(e)}")
            metrics.record_failure(f"similarity_{content_id1}_{content_id2}", time.time() - start_time)

def test_user_preferences(recommender: ContentRecommendationSystem,
                         users: Dict[str, Dict[str, Any]],
                         content: Dict[str, Dict[str, Any]],
                         metrics: RecommendationTestMetrics) -> None:
    """Test personalized recommendations based on user preferences"""
    print("\n👤 Testing User Preferences:")
    
    for user_id, user_data in users.items():
        start_time = time.time()
        try:
            # Get personalized recommendations
            recommendations = recommender.process({
                'user_id': user_id,
                'user_profile': user_data['profile'],
                'mode': 'personalized'
            }).results
            
            # Analyze recommendation relevance
            relevance_scores = []
            for rec in recommendations:
                content_data = content[rec['content_id']]
                
                # Check topic overlap
                topic_match = len(set(content_data['topics']).intersection(user_data['profile']['interests'])) > 0
                
                # Check complexity match
                complexity_diff = abs(content_data['complexity'] - user_data['profile']['preferred_complexity'])
                complexity_match = complexity_diff < 0.3
                
                relevance_scores.append(float(topic_match and complexity_match))
            
            relevance = sum(relevance_scores) / len(relevance_scores) if relevance_scores else 0
            
            print(f"  ✅ User: {user_id}")
            print(f"    - Recommendation Relevance: {relevance:.2%}")
            
            metrics.record_success('user_preferences', relevance, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed preferences test for {user_id}: {str(e)}")
            metrics.record_failure(f"preferences_{user_id}", time.time() - start_time)

def test_recommendation_diversity(recommender: ContentRecommendationSystem,
                                users: Dict[str, Dict[str, Any]],
                                content: Dict[str, Dict[str, Any]],
                                metrics: RecommendationTestMetrics) -> None:
    """Test diversity in recommendations"""
    print("\n🎯 Testing Recommendation Diversity:")
    
    for user_id, user_data in users.items():
        start_time = time.time()
        try:
            # Get recommendations
            recommendations = recommender.process({
                'user_id': user_id,
                'user_profile': user_data['profile'],
                'mode': 'diverse'
            }).results
            
            # Analyze diversity
            topics = set()
            complexities = []
            timestamps = []
            
            for rec in recommendations:
                content_data = content[rec['content_id']]
                topics.update(content_data['topics'])
                complexities.append(content_data['complexity'])
                timestamps.append(content_data['timestamp'].timestamp())
            
            # Calculate diversity metrics
            topic_diversity = len(topics) / (len(recommendations) * 2)  # Each content has 2 topics
            complexity_diversity = np.std(complexities) if complexities else 0
            temporal_diversity = np.std(timestamps) if timestamps else 0
            
            # Combine diversity metrics
            diversity_score = (topic_diversity + min(1.0, complexity_diversity) + min(1.0, temporal_diversity / (30 * 24 * 3600))) / 3
            
            print(f"  ✅ User: {user_id}")
            print(f"    - Topic Diversity: {topic_diversity:.2%}")
            print(f"    - Complexity Spread: {complexity_diversity:.2f}")
            
            metrics.record_success('diversity', diversity_score, time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed diversity test for {user_id}: {str(e)}")
            metrics.record_failure(f"diversity_{user_id}", time.time() - start_time)

def generate_markdown_report(summary: Dict[str, Any]) -> str:
    """Generate a detailed markdown report of the recommendation test results"""
    report = [
        "# Content Recommendation System Test Results",
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
    """Main test runner for recommendation system"""
    print("🚀 Starting Content Recommendation System Tests")
    
    # Initialize metrics and recommendation system
    metrics = RecommendationTestMetrics()
    recommender = ContentRecommendationSystem(db_config=TestDatabaseConfig())
    
    try:
        # Generate test data
        content = generate_test_content()
        users = generate_test_users()
        
        # Add content to recommendation system
        for content_data in content.values():
            recommender.add_content(
                content_id=content_data['content_id'],
                text=content_data['text'],
                timestamp=content_data['timestamp'].timestamp(),
                authenticity_score=content_data['authenticity'],
                metadata={
                    'topics': content_data['topics'],
                    'complexity': content_data['complexity'],
                    'length': content_data['length']
                }
            )
        
        # Run tests
        test_content_similarity(recommender, content, metrics)
        test_user_preferences(recommender, users, content, metrics)
        test_recommendation_diversity(recommender, users, content, metrics)
        
        # Get test summary
        summary = metrics.get_summary()
        
        if report_mode:
            return summary
        
        # Generate and save standalone report
        report = generate_markdown_report(summary)
        report_path = Path(__file__).parent / 'recommendation_results.md'
        with open(report_path, 'w') as f:
            f.write(report)
        
        print("\n📊 Test Results Summary:")
        print(f"- Success Rate: {summary['success_rate']:.2%}")
        print(f"- Total Time: {summary['total_execution_time']:.2f} seconds")
        print(f"\nDetailed results written to: {report_path}")
        
    except Exception as e:
        print(f"\n❌ Test suite failed: {str(e)}")
        raise
    
    print("\n✅ Recommendation tests completed")

if __name__ == "__main__":
    main() 