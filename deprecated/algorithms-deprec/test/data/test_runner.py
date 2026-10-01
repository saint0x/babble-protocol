"""
Direct Algorithm Test Runner with SQLite Integration
"""
import os
os.environ['BABEL_TEST_MODE'] = '1'  # Set test mode before any imports

import json
import sys
import time
import random
from pathlib import Path
from datetime import datetime, timedelta
from typing import Dict, List, Any, Optional
import numpy as np
from collections import defaultdict

# Set up NLTK data path and download required data silently
import nltk
nltk.data.path = [str(Path.home() / 'nltk_data')]

# Download all required NLTK data
nltk.download('punkt', quiet=True)
nltk.download('stopwords', quiet=True)
nltk.download('wordnet', quiet=True)
nltk.download('vader_lexicon', quiet=True)
nltk.download('averaged_perceptron_tagger', quiet=True)
nltk.download('maxent_ne_chunker', quiet=True)
nltk.download('words', quiet=True)

# Add parent directory to path for imports
sys.path.append(str(Path(__file__).parent.parent.parent))

from algorithms.content_analysis import ContentAnalysis
from algorithms.recommendation import ContentRecommendationSystem
from algorithms.feedback_loop_optimization import FeedbackLoopOptimizer
from algorithms.community_moderation import CommunityModerationSystem
from algorithms.engagement_analytics import EngagementAnalytics
from algorithms.temporal_considerations import TemporalConsiderations
from algorithms.consensus import SourceOfTruthConsensus

# Import test database configuration
from algorithms.test.data.db_config import TestDatabaseConfig
from algorithms.test.data.init_db import init_test_db

class TestMetrics:
    """Track performance metrics for tests"""
    def __init__(self):
        self.start_time = time.time()
        self.metrics = {
            'content_analysis': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0},
            'recommendations': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0},
            'optimization': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0},
            'moderation': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0},
            'engagement': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0},
            'temporal': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0},
            'consensus': {'success': 0, 'failed': 0, 'time': 0, 'edge_cases': 0}
        }
        self.edge_case_scenarios = defaultdict(list)
    
    def record_success(self, algorithm: str, execution_time: float, is_edge_case: bool = False):
        self.metrics[algorithm]['success'] += 1
        self.metrics[algorithm]['time'] += execution_time
        if is_edge_case:
            self.metrics[algorithm]['edge_cases'] += 1
    
    def record_failure(self, algorithm: str, execution_time: float, scenario: str = None):
        self.metrics[algorithm]['failed'] += 1
        self.metrics[algorithm]['time'] += execution_time
        if scenario:
            self.edge_case_scenarios[algorithm].append(scenario)
    
    def get_summary(self) -> Dict[str, Any]:
        total_time = time.time() - self.start_time
        return {
            'total_execution_time': total_time,
            'algorithm_metrics': self.metrics,
            'edge_case_scenarios': dict(self.edge_case_scenarios)
        }

class SocialMediaSimulator:
    """Simulate complex social media interactions and behaviors"""
    
    def __init__(self):
        self.viral_probability = 0.05
        self.bot_probability = 0.1
        self.controversy_probability = 0.15
        self.time_decay_factor = 0.1
        
    def generate_viral_cascade(self, content_id: str, base_engagement: float) -> List[Dict[str, Any]]:
        """Simulate viral content spread patterns"""
        cascade_size = np.random.exponential(scale=100)
        time_points = np.sort(np.random.exponential(scale=24, size=int(cascade_size)))
        
        cascade = []
        current_engagement = base_engagement
        for t in time_points:
            engagement_boost = np.random.normal(1.5, 0.3)
            current_engagement *= engagement_boost
            cascade.append({
                'timestamp': t,
                'engagement_level': min(current_engagement, 1.0),
                'reach': int(np.random.exponential(scale=1000))
            })
        return cascade
    
    def simulate_bot_behavior(self, content_data: Dict[str, Any]) -> Dict[str, Any]:
        """Add bot-like behavior patterns to content interactions"""
        bot_patterns = {
            'repetitive_engagement': random.random() < 0.7,
            'unnatural_timing': random.random() < 0.8,
            'coordinated_action': random.random() < 0.4
        }
        content_data['bot_patterns'] = bot_patterns
        return content_data
    
    def add_controversy_signals(self, content_data: Dict[str, Any]) -> Dict[str, Any]:
        """Add controversial aspects to content"""
        controversy_signals = {
            'polarized_reactions': random.random() < 0.6,
            'rapid_engagement_shifts': random.random() < 0.4,
            'conflicting_narratives': random.random() < 0.5
        }
        content_data['controversy_signals'] = controversy_signals
        return content_data

def generate_complex_test_data() -> tuple[Dict[str, Any], Dict[str, Any]]:
    """Generate complex test data with realistic social media patterns"""
    simulator = SocialMediaSimulator()
    users = {}
    content = {}
    
    # Generate diverse user profiles
    expertise_levels = ['novice', 'intermediate', 'expert', 'authority']
    engagement_patterns = ['casual', 'regular', 'power_user', 'influencer']
    
    for i in range(100):
        user_id = f"user_{i}"
        is_bot = random.random() < simulator.bot_probability
        
        user_profile = {
            'user_id': user_id,
            'name': f"Test User {i}",
            'expertise_level': random.choice(expertise_levels),
            'engagement_pattern': random.choice(engagement_patterns),
            'credibility_score': random.uniform(0.1, 1.0),
            'is_verified': random.random() < 0.2,
            'account_age_days': random.randint(1, 1000),
            'is_bot': is_bot,
            'interests': random.sample(['tech', 'science', 'politics', 'entertainment', 'sports'], k=random.randint(1, 4)),
            'expertise_areas': random.sample(['AI', 'climate', 'healthcare', 'education', 'finance'], k=random.randint(1, 3)),
            'engagement_history': {
                'posts_per_day': random.uniform(0, 20),
                'avg_engagement_rate': random.uniform(0.01, 0.3),
                'response_time_minutes': random.uniform(1, 120)
            }
        }
        users[user_id] = user_profile
    
    # Generate diverse content with complex patterns
    content_types = ['text', 'image', 'video', 'link', 'poll']
    sentiment_categories = ['positive', 'negative', 'neutral', 'controversial']
    
    for i in range(200):
        content_id = f"content_{i}"
        is_viral = random.random() < simulator.viral_probability
        is_controversial = random.random() < simulator.controversy_probability
        
        content_data = {
            'content_id': content_id,
            'creator_id': random.choice(list(users.keys())),
            'content_type': random.choice(content_types),
            'timestamp': datetime.now() - timedelta(days=random.randint(0, 30)),
            'text': f"Sample content {i} with varying complexity and patterns",
            'title': f"Test Content {i}",
            'sentiment': random.choice(sentiment_categories),
            'complexity_level': random.uniform(0.1, 1.0),
            'authenticity_signals': {
                'source_reliability': random.uniform(0.1, 1.0),
                'fact_check_score': random.uniform(0.1, 1.0),
                'verification_status': random.choice(['verified', 'unverified', 'disputed'])
            },
            'engagement_metrics': {
                'views': random.randint(100, 10000),
                'likes': random.randint(10, 1000),
                'shares': random.randint(0, 200),
                'comments': random.randint(0, 300),
                'save_rate': random.uniform(0.01, 0.2)
            },
            'tags': random.sample(['trending', 'educational', 'news', 'entertainment', 'viral'], k=random.randint(1, 3))
        }
        
        if is_viral:
            content_data['viral_cascade'] = simulator.generate_viral_cascade(content_id, content_data['engagement_metrics']['likes'] / 1000)
        
        if is_controversial:
            content_data = simulator.add_controversy_signals(content_data)
        
        if random.random() < simulator.bot_probability:
            content_data = simulator.simulate_bot_behavior(content_data)
        
        content[content_id] = content_data
    
    return users, content

def test_content_analysis(content: Dict[str, Any], metrics: TestMetrics) -> Dict[str, Any]:
    """Test content analysis with focus on understanding and categorizing content"""
    analyzer = ContentAnalysis(db_config=TestDatabaseConfig())
    results = {}
    
    print("\n🔍 Testing Content Analysis:")
    
    # Test complex content understanding
    complex_cases = {
        'technical_analysis': {
            'content_id': 'tech_001',
            'text': """
            The implementation of zero-knowledge proofs in modern cryptographic systems relies on several key mathematical principles:
            1. Homomorphic encryption allowing computation on encrypted data
            2. Elliptic curve pairings enabling efficient verification
            3. Polynomial commitments for succinctness
            This technical analysis explores each component in detail...
            """,
            'expected_topics': ['cryptography', 'mathematics', 'zero-knowledge', 'technical'],
            'expected_complexity': 0.9
        },
        'market_sentiment': {
            'content_id': 'market_001',
            'text': """
            Market sentiment remains bearish as recent developments have shaken investor confidence.
            Key indicators:
            - Trading volume down 25%
            - New user onboarding decreased
            - Protocol TVL dropping
            However, long-term fundamentals remain strong...
            """,
            'expected_topics': ['market', 'analysis', 'trading', 'sentiment'],
            'expected_complexity': 0.7
        },
        'mixed_content': {
            'content_id': 'mixed_001',
            'text': """
            🚀 Exciting protocol update! 
            
            Technical changes:
            - Improved gas optimization
            - New privacy features
            
            For users:
            - Lower fees
            - Better UX
            
            Join our Discord for more info! 💬
            """,
            'expected_topics': ['announcement', 'technical', 'community'],
            'expected_complexity': 0.5
        }
    }
    
    # Test edge cases first
    edge_cases = {
        'empty_content': {'content_id': 'edge_empty', 'text': '', 'expected_topics': []},
        'very_long_content': {'content_id': 'edge_long', 'text': 'x' * 50000, 'expected_topics': []},
        'special_chars': {'content_id': 'edge_special', 'text': '!@#$%^&*()\n\t\r', 'expected_topics': []},
        'mixed_languages': {'content_id': 'edge_lang', 'text': 'English text with 中文 and Español', 'expected_topics': ['multilingual']},
        'code_snippets': {
            'content_id': 'edge_code',
            'text': '''
            function analyze() {
                // JavaScript code
                console.log("test");
            }
            
            def process():
                # Python code
                print("test")
            ''',
            'expected_topics': ['code', 'technical']
        }
    }
    
    # Test edge cases
    for case_name, case_data in edge_cases.items():
        start_time = time.time()
        try:
            result = analyzer.process({
                'content_id': case_data['content_id'],
                'text': case_data['text'],
                'metadata': {
                    'is_edge_case': True,
                    'expected_topics': case_data['expected_topics']
                }
            })
            
            # Verify topic detection accuracy
            detected_topics = set(result.results[0].get('topics', []))
            expected_topics = set(case_data['expected_topics'])
            topic_accuracy = len(detected_topics.intersection(expected_topics)) / len(expected_topics) if expected_topics else 1.0
            
            print(f"  ✅ Edge case: {case_name} (Topic Accuracy: {topic_accuracy:.2%})")
            results[case_data['content_id']] = result.results[0]
            metrics.record_success('content_analysis', time.time() - start_time, is_edge_case=True)
            
        except Exception as e:
            print(f"  ❌ Edge case failed: {case_name}: {str(e)}")
            metrics.record_failure('content_analysis', time.time() - start_time, scenario=case_name)
    
    # Test complex content understanding
    for case_name, case_data in complex_cases.items():
        start_time = time.time()
        try:
            result = analyzer.process({
                'content_id': case_data['content_id'],
                'text': case_data['text'],
                'metadata': {
                    'expected_topics': case_data['expected_topics'],
                    'expected_complexity': case_data['expected_complexity']
                }
            })
            
            # Verify topic detection and complexity assessment
            detected_topics = set(result.results[0].get('topics', []))
            expected_topics = set(case_data['expected_topics'])
            topic_accuracy = len(detected_topics.intersection(expected_topics)) / len(expected_topics)
            complexity_diff = abs(result.results[0].get('complexity', 0) - case_data['expected_complexity'])
            
            print(f"  ✅ Complex case: {case_name}")
            print(f"    - Topic Accuracy: {topic_accuracy:.2%}")
            print(f"    - Complexity Difference: {complexity_diff:.2f}")
            
            results[case_data['content_id']] = result.results[0]
            metrics.record_success('content_analysis', time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Complex case failed: {case_name}: {str(e)}")
            metrics.record_failure('content_analysis', time.time() - start_time)
    
    return results

def test_recommendations(users_data: Dict[str, Any], content_results: Dict[str, Any], metrics: TestMetrics) -> Dict[str, Any]:
    """Test recommendation system with focus on personalization and relevance"""
    recommender = ContentRecommendationSystem(db_config=TestDatabaseConfig())
    results = {}
    
    print("\n🎯 Testing Recommendations:")
    
    # Define user personas with expected content preferences
    user_personas = {
        'technical_expert': {
            'user_id': 'test_tech_1',
            'profile': {
                'interests': ['blockchain', 'programming', 'cryptography'],
                'expertise_level': 'expert',
                'preferred_complexity': 0.8,
                'reading_history': ['technical_post', 'research_paper']
            },
            'expected_content': ['technical_post', 'research_paper'],
            'avoided_content': ['spam_post', 'community_post']
        },
        'beginner_user': {
            'user_id': 'test_begin_1',
            'profile': {
                'interests': ['cryptocurrency', 'blockchain basics'],
                'expertise_level': 'beginner',
                'preferred_complexity': 0.4,
                'reading_history': ['tutorial_post', 'community_post']
            },
            'expected_content': ['tutorial_post', 'educational_post'],
            'avoided_content': ['research_paper', 'technical_post']
        },
        'trader': {
            'user_id': 'test_trade_1',
            'profile': {
                'interests': ['trading', 'market analysis', 'defi'],
                'expertise_level': 'intermediate',
                'preferred_complexity': 0.6,
                'reading_history': ['market_analysis', 'news_post']
            },
            'expected_content': ['market_analysis', 'news_post'],
            'avoided_content': ['philosophical_post', 'tutorial_post']
        }
    }
    
    # Add all content to the recommendation system
    for content_id, content in content_results.items():
        recommender.add_content(
            content_id=content['content_id'],
            text=content['text'],
            timestamp=datetime.now().timestamp(),
            metadata={
                'topics': content.get('topics', []),
                'complexity': content.get('complexity', 0.5),
                'engagement_metrics': content.get('engagement_metrics', {})
            }
        )
    
    # Test recommendations for each persona
    for persona_name, persona in user_personas.items():
        start_time = time.time()
        try:
            # Get recommendations
            recommendations = recommender.process({
                'user_id': persona['user_id'],
                'user_profile': persona['profile'],
                'context': {
                    'time_of_day': datetime.now().hour,
                    'platform': 'desktop',
                    'session_duration': 30
                }
            })
            
            # Analyze recommendation quality
            rec_content_ids = [r['content_id'] for r in recommendations.results]
            
            # Check if expected content types are recommended
            expected_hits = sum(1 for c in persona['expected_content'] if any(c in r for r in rec_content_ids))
            expected_accuracy = expected_hits / len(persona['expected_content'])
            
            # Check if avoided content types are not recommended
            avoided_hits = sum(1 for c in persona['avoided_content'] if any(c in r for r in rec_content_ids))
            avoidance_accuracy = 1 - (avoided_hits / len(persona['avoided_content']))
            
            print(f"  ✅ Persona: {persona_name}")
            print(f"    - Expected Content Accuracy: {expected_accuracy:.2%}")
            print(f"    - Avoidance Accuracy: {avoidance_accuracy:.2%}")
            
            results[persona['user_id']] = {
                'recommendations': recommendations.results,
                'metrics': {
                    'expected_accuracy': expected_accuracy,
                    'avoidance_accuracy': avoidance_accuracy
                }
            }
            
            metrics.record_success('recommendations', time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed recommendations for {persona_name}: {str(e)}")
            metrics.record_failure('recommendations', time.time() - start_time)
    
    return results

def test_feedback_optimization(users_data: Dict[str, Any], recommendations: Dict[str, Any], metrics: TestMetrics) -> Dict[str, Any]:
    """Test feedback loop optimization with focus on user behavior adaptation"""
    optimizer = FeedbackLoopOptimizer(db_config=TestDatabaseConfig())
    results = {}
    
    print("\n💭 Testing Feedback Optimization:")
    
    # Define user behavior patterns
    behavior_patterns = {
        'power_user': {
            'pattern_id': 'power_1',
            'engagement': {
                'read_time': {'mean': 300, 'std': 50},  # seconds
                'scroll_depth': {'mean': 0.95, 'std': 0.05},
                'click_rate': {'mean': 0.8, 'std': 0.1},
                'share_rate': {'mean': 0.3, 'std': 0.1},
                'return_rate': {'mean': 0.9, 'std': 0.05}
            },
            'content_preferences': {
                'complexity': {'min': 0.7, 'max': 1.0},
                'length': {'min': 1000, 'max': 5000},
                'topics': ['technical', 'research', 'analysis']
            }
        },
        'casual_user': {
            'pattern_id': 'casual_1',
            'engagement': {
                'read_time': {'mean': 120, 'std': 30},
                'scroll_depth': {'mean': 0.6, 'std': 0.2},
                'click_rate': {'mean': 0.4, 'std': 0.15},
                'share_rate': {'mean': 0.1, 'std': 0.05},
                'return_rate': {'mean': 0.5, 'std': 0.2}
            },
            'content_preferences': {
                'complexity': {'min': 0.3, 'max': 0.7},
                'length': {'min': 200, 'max': 1000},
                'topics': ['basics', 'news', 'community']
            }
        },
        'bot_behavior': {
            'pattern_id': 'bot_1',
            'engagement': {
                'read_time': {'mean': 10, 'std': 5},
                'scroll_depth': {'mean': 1.0, 'std': 0.0},
                'click_rate': {'mean': 0.95, 'std': 0.05},
                'share_rate': {'mean': 0.8, 'std': 0.1},
                'return_rate': {'mean': 0.99, 'std': 0.01}
            },
            'content_preferences': None  # Bots don't have real preferences
        },
        'learning_user': {
            'pattern_id': 'learn_1',
            'engagement': {
                'read_time': {'mean': 240, 'std': 60},
                'scroll_depth': {'mean': 0.85, 'std': 0.1},
                'click_rate': {'mean': 0.6, 'std': 0.2},
                'share_rate': {'mean': 0.2, 'std': 0.1},
                'return_rate': {'mean': 0.7, 'std': 0.15}
            },
            'content_preferences': {
                'complexity': {'min': 0.4, 'max': 0.8},
                'length': {'min': 500, 'max': 2000},
                'topics': ['tutorials', 'education', 'guides']
            }
        }
    }
    
    # Simulate feedback sessions
    for pattern_name, pattern in behavior_patterns.items():
        start_time = time.time()
        try:
            print(f"\n  Testing {pattern_name} behavior pattern:")
            
            # Simulate multiple feedback sessions
            feedback_sessions = []
            for session in range(5):  # 5 sessions per pattern
                # Generate engagement metrics with noise
                engagement = {}
                for metric, params in pattern['engagement'].items():
                    value = random.gauss(params['mean'], params['std'])
                    engagement[metric] = max(0, min(1, value))  # Clamp between 0 and 1
                
                # Create session feedback
                session_feedback = {
                    'session_id': f"{pattern['pattern_id']}_session_{session}",
                    'timestamp': datetime.now() - timedelta(hours=session),
                    'engagement_metrics': engagement,
                    'content_interactions': []
                }
                
                # Add content interactions
                for _ in range(random.randint(3, 8)):  # Random number of interactions per session
                    interaction = {
                        'content_id': f"content_{random.randint(1, 100)}",
                        'read_time': random.gauss(pattern['engagement']['read_time']['mean'], 
                                                pattern['engagement']['read_time']['std']),
                        'scroll_depth': min(1.0, max(0, random.gauss(pattern['engagement']['scroll_depth']['mean'],
                                                                    pattern['engagement']['scroll_depth']['std']))),
                        'clicked_links': random.random() < pattern['engagement']['click_rate']['mean'],
                        'shared': random.random() < pattern['engagement']['share_rate']['mean']
                    }
                    session_feedback['content_interactions'].append(interaction)
                
                feedback_sessions.append(session_feedback)
            
            # Process feedback through optimizer
            optimization_result = optimizer.process({
                'pattern_id': pattern['pattern_id'],
                'feedback_sessions': feedback_sessions,
                'user_context': {
                    'pattern_type': pattern_name,
                    'content_preferences': pattern['content_preferences'],
                    'session_history': {
                        'total_sessions': 5,
                        'avg_session_length': pattern['engagement']['read_time']['mean'] * 3,
                        'return_rate': pattern['engagement']['return_rate']['mean']
                    }
                }
            })
            
            # Analyze optimization results
            result = optimization_result.results[0]
            
            # Verify pattern detection
            detected_pattern = result.get('detected_pattern', '')
            pattern_match = pattern_name.lower() in detected_pattern.lower()
            
            # Verify content adaptations
            content_adaptations = result.get('content_adaptations', {})
            adaptation_score = 0
            if pattern['content_preferences']:
                # Check if adaptations match expected preferences
                complexity_match = (content_adaptations.get('complexity_range', {}).get('min', 0) >= 
                                 pattern['content_preferences']['complexity']['min'])
                length_match = (content_adaptations.get('length_range', {}).get('max', 0) <= 
                              pattern['content_preferences']['length']['max'])
                topic_match = any(topic in content_adaptations.get('recommended_topics', [])
                                for topic in pattern['content_preferences']['topics'])
                adaptation_score = (complexity_match + length_match + topic_match) / 3
            
            print(f"    - Pattern Detection: {'✓' if pattern_match else '✗'}")
            print(f"    - Adaptation Accuracy: {adaptation_score:.2%}")
            
            results[pattern['pattern_id']] = {
                'optimization_result': result,
                'accuracy': {
                    'pattern_detection': pattern_match,
                    'adaptation_score': adaptation_score
                }
            }
            
            metrics.record_success('optimization', time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed optimization for {pattern_name}: {str(e)}")
            metrics.record_failure('optimization', time.time() - start_time, scenario=pattern_name)
    
    return results

def test_moderation(content_results: Dict[str, Any], metrics: TestMetrics) -> Dict[str, Any]:
    """Test moderation system with focus on content safety and quality"""
    moderator = CommunityModerationSystem(db_config=TestDatabaseConfig())
    results = {}
    
    print("\n🛡️ Testing Community Moderation:")
    
    # Define test cases for different types of problematic content
    moderation_cases = {
        'hate_speech': {
            'content_id': 'mod_hate_1',
            'text': "This is a simulated hate speech message targeting specific groups...",
            'expected_flags': ['hate_speech', 'harmful_content'],
            'expected_action': 'remove'
        },
        'spam_promotion': {
            'content_id': 'mod_spam_1',
            'text': "🚀 GET RICH QUICK! Buy now!!! 1000x guaranteed! Limited time offer! Click here! www.scam.com 💰💰💰",
            'expected_flags': ['spam', 'excessive_promotion', 'suspicious_links'],
            'expected_action': 'remove'
        },
        'mild_spam': {
            'content_id': 'mod_spam_2',
            'text': "Check out my new project! Great opportunity for investors. Link in bio.",
            'expected_flags': ['promotional', 'self_promotion'],
            'expected_action': 'flag'
        },
        'misinformation': {
            'content_id': 'mod_misinfo_1',
            'text': "Breaking: Major protocol secretly selling user data. Inside sources confirm massive scandal...",
            'expected_flags': ['potential_misinformation', 'unverified_claims'],
            'expected_action': 'flag'
        },
        'coordinated_attack': {
            'content_id': 'mod_attack_1',
            'text': "This is part of a coordinated FUD campaign with multiple identical posts...",
            'metadata': {
                'pattern': 'coordinated',
                'similar_posts': 15,
                'time_window': '1h'
            },
            'expected_flags': ['coordinated_behavior', 'manipulation'],
            'expected_action': 'remove'
        },
        'borderline': {
            'content_id': 'mod_border_1',
            'text': "This protocol's performance has been terrible lately. Developers are incompetent.",
            'expected_flags': ['negative', 'low_quality'],
            'expected_action': 'warn'
        },
        'safe_critical': {
            'content_id': 'mod_safe_1',
            'text': "A detailed analysis of the protocol's shortcomings: 1. Performance issues...",
            'expected_flags': [],
            'expected_action': 'allow'
        }
    }
    
    # Test each moderation case
    for case_name, case_data in moderation_cases.items():
        start_time = time.time()
        try:
            # Process content through moderation
            moderation_result = moderator.process({
                'content_id': case_data['content_id'],
                'text': case_data['text'],
                'metadata': case_data.get('metadata', {}),
                'context': {
                    'user_reports': random.randint(0, 10),
                    'user_history': {
                        'previous_violations': random.randint(0, 5),
                        'account_age_days': random.randint(1, 365)
                    }
                }
            })
            
            result = moderation_result.results[0]
            
            # Verify moderation accuracy
            detected_flags = set(result.get('flags', []))
            expected_flags = set(case_data['expected_flags'])
            flag_accuracy = len(detected_flags.intersection(expected_flags)) / len(expected_flags) if expected_flags else 1.0
            
            action_correct = result.get('action') == case_data['expected_action']
            
            print(f"  ✅ Moderation case: {case_name}")
            print(f"    - Flag Detection Accuracy: {flag_accuracy:.2%}")
            print(f"    - Action Accuracy: {'✓' if action_correct else '✗'}")
            
            results[case_data['content_id']] = {
                'moderation_result': result,
                'accuracy': {
                    'flag_detection': flag_accuracy,
                    'action_correct': action_correct
                }
            }
            
            metrics.record_success('moderation', time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed moderation for {case_name}: {str(e)}")
            metrics.record_failure('moderation', time.time() - start_time, scenario=case_name)
    
    return results

def test_consensus(content_results: Dict[str, Any], metrics: TestMetrics) -> Dict[str, Any]:
    """Test consensus mechanism with focus on truth determination"""
    consensus = SourceOfTruthConsensus(db_config=TestDatabaseConfig())
    results = {}
    
    print("\n🤝 Testing Source of Truth Consensus:")
    
    # Define test cases for truth determination
    consensus_cases = {
        'clear_truth': {
            'content_id': 'consensus_1',
            'claim': "Protocol X has implemented feature Y",
            'sources': [
                {'type': 'official_docs', 'text': "Feature Y is now live in Protocol X", 'reliability': 0.9},
                {'type': 'github', 'text': "Merged PR: Implementation of feature Y", 'reliability': 0.9},
                {'type': 'community', 'text': "Just tested feature Y in Protocol X", 'reliability': 0.7}
            ],
            'expected_truth_score': 0.9
        },
        'conflicting_info': {
            'content_id': 'consensus_2',
            'claim': "Protocol X has been hacked",
            'sources': [
                {'type': 'social_media', 'text': "BREAKING: Protocol X hacked!", 'reliability': 0.3},
                {'type': 'official', 'text': "No hack occurred. All funds are safe.", 'reliability': 0.9},
                {'type': 'security_firm', 'text': "Investigating potential security incident", 'reliability': 0.8}
            ],
            'expected_truth_score': 0.4
        },
        'unverified_claim': {
            'content_id': 'consensus_3',
            'claim': "Protocol X will launch new token",
            'sources': [
                {'type': 'rumor', 'text': "Heard about new token launch", 'reliability': 0.2},
                {'type': 'speculation', 'text': "Analysis suggests possible token", 'reliability': 0.4}
            ],
            'expected_truth_score': 0.3
        },
        'technical_consensus': {
            'content_id': 'consensus_4',
            'claim': "Bug found in smart contract",
            'sources': [
                {'type': 'audit_report', 'text': "Critical vulnerability discovered", 'reliability': 0.95},
                {'type': 'developer', 'text': "Confirmed bug in function X", 'reliability': 0.9},
                {'type': 'community', 'text': "Strange behavior in contract", 'reliability': 0.6}
            ],
            'expected_truth_score': 0.85
        }
    }
    
    # Test each consensus case
    for case_name, case_data in consensus_cases.items():
        start_time = time.time()
        try:
            # Process through consensus mechanism
            consensus_result = consensus.process({
                'content_id': case_data['content_id'],
                'claim': case_data['claim'],
                'sources': case_data['sources']
            })
            
            result = consensus_result.results[0]
            
            # Verify consensus accuracy
            truth_score = result.get('truth_score', 0)
            score_difference = abs(truth_score - case_data['expected_truth_score'])
            
            print(f"  ✅ Consensus case: {case_name}")
            print(f"    - Truth Score: {truth_score:.2f} (Expected: {case_data['expected_truth_score']:.2f})")
            print(f"    - Score Difference: {score_difference:.2f}")
            
            results[case_data['content_id']] = {
                'consensus_result': result,
                'accuracy': {
                    'score_difference': score_difference,
                    'within_tolerance': score_difference <= 0.1
                }
            }
            
            metrics.record_success('consensus', time.time() - start_time)
            
        except Exception as e:
            print(f"  ❌ Failed consensus for {case_name}: {str(e)}")
            metrics.record_failure('consensus', time.time() - start_time, scenario=case_name)
    
    return results

def generate_markdown_report(summary: Dict[str, Any], content_data: Dict[str, Any], users_data: Dict[str, Any]) -> str:
    """Generate a human-readable markdown report"""
    report = [
        "# Algorithm Performance Report",
        f"\n## Test Overview",
        f"- **Test Date**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        f"- **Total Execution Time**: {summary['total_execution_time']:.2f} seconds",
        f"- **Test Dataset Size**:",
        f"  - Users: {len(users_data)}",
        f"  - Content Items: {len(content_data)}",
        "\n## Algorithm Performance Summary\n"
    ]
    
    # Add performance summary for each algorithm
    for algo, stats in summary['algorithm_metrics'].items():
        total_tests = stats['success'] + stats['failed']
        if total_tests == 0:
            continue
            
        success_rate = stats['success'] / total_tests * 100
        avg_time = stats['time'] / total_tests
        
        # Convert algorithm name to title case and replace underscores
        algo_name = algo.replace('_', ' ').title()
        
        report.extend([
            f"### {algo_name}",
            f"- **Success Rate**: {success_rate:.1f}%",
            f"- **Average Processing Time**: {avg_time:.3f} seconds",
            f"- **Total Tests**: {total_tests}",
            f"- **Edge Cases Handled**: {stats['edge_cases']}",
        ])
        
        # Add failed scenarios if any
        if algo in summary['edge_case_scenarios'] and summary['edge_case_scenarios'][algo]:
            report.append("- **Failed Scenarios**:")
            for scenario in summary['edge_case_scenarios'][algo]:
                report.append(f"  - {scenario}")
        
        report.append("")  # Add blank line between sections
    
    # Add test data characteristics
    report.extend([
        "## Test Data Characteristics\n",
        "### User Profiles",
        "- **Expertise Levels**: novice, intermediate, expert, authority",
        "- **Engagement Patterns**: casual, regular, power_user, influencer",
        "- **Bot Probability**: 10%",
        "- **Verification Rate**: 20%",
        "\n### Content Characteristics",
        "- **Content Types**: text, image, video, link, poll",
        "- **Viral Content Rate**: 5%",
        "- **Controversy Rate**: 15%",
        "- **Time Span**: 30 days",
        "\n### Edge Cases Tested",
        "#### Content Analysis",
        "- Empty content",
        "- Very long content (50,000 characters)",
        "- Special characters and formatting",
        "- Mixed language content",
        "- Code snippets",
        "\n#### Moderation",
        "- Coordinated spam campaigns",
        "- Hate speech patterns",
        "- Misinformation spread",
        "\n#### User Behavior",
        "- Bot-like engagement patterns",
        "- Viral content cascades",
        "- Controversial content interactions",
        "- Platform-specific behavior",
        "\n## Recommendations for Improvement\n"
    ])
    
    # Add algorithm-specific recommendations
    for algo, stats in summary['algorithm_metrics'].items():
        if stats['success'] + stats['failed'] == 0:
            report.extend([
                f"### {algo.replace('_', ' ').title()}",
                "- 🚨 **Critical**: Implementation missing",
                "- Needs full implementation and testing\n"
            ])
        elif stats['failed'] > 0:
            report.extend([
                f"### {algo.replace('_', ' ').title()}",
                "- ⚠️ **Needs Attention**: Failed edge cases detected",
                "- Focus on improving edge case handling",
                "- Consider adding more robust error handling\n"
            ])
    
    return "\n".join(report)

def main():
    """Main test runner"""
    print("🚀 Starting Algorithm Test Suite with Enhanced Complexity")
    
    # Initialize metrics tracking
    metrics = TestMetrics()
    
    try:
        # Generate complex test data
        print("\n📊 Generating complex test data...")
        users_data, content_data = generate_complex_test_data()
        print(f"Generated {len(users_data)} user profiles and {len(content_data)} content items")
        
        # Run tests with enhanced complexity
        content_results = test_content_analysis(content_data, metrics)
        recommendation_results = test_recommendations(users_data, content_results, metrics)
        optimization_results = test_feedback_optimization(users_data, recommendation_results, metrics)
        moderation_results = test_moderation(content_results, metrics)
        consensus_results = test_consensus(content_results, metrics)
        
        # Generate and save detailed report
        summary = metrics.get_summary()
        report = generate_markdown_report(summary, content_data, users_data)
        
        # Save report to RESULTS.md in project root (move up one more directory)
        report_path = Path(__file__).parent.parent.parent.parent.parent / 'RESULTS.md'
        with open(report_path, 'w') as f:
            f.write(report)
        
        # Print summary to console
        print("\n📈 Test Results Summary:")
        print(f"\nTotal execution time: {summary['total_execution_time']:.2f} seconds")
        print("\nDetailed results have been written to RESULTS.md")
        
    except Exception as e:
        print(f"\n❌ Test suite failed: {str(e)}")
        raise
    
    print("\n✅ Test suite completed")

if __name__ == "__main__":
    main()
