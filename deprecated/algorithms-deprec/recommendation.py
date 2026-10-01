"""
Enhanced Content Recommendation System

This module implements a content recommendation system that combines collaborative 
filtering, content-based filtering, and feedback loop optimization.
"""

import time
from typing import Dict, Any, List, Optional
from datetime import datetime
import numpy as np
from sklearn.metrics.pairwise import cosine_similarity
from pydantic import BaseModel

from .base import BaseAlgorithm, AlgorithmResponse, AlgorithmMetrics

class UserProfile(BaseModel):
    """User profile for recommendation"""
    user_id: str
    interests: List[str]
    expertise_areas: List[str]
    engagement_history: List[Dict[str, Any]]
    feature_vector: Optional[List[float]] = None
    
class ContentVector(BaseModel):
    """Content vector for recommendation"""
    content_id: str
    topics: List[str]
    complexity_level: float
    feature_vector: List[float]

class RecommendationScore(BaseModel):
    """Recommendation score model"""
    content_id: str
    relevance_score: float
    engagement_score: float
    authenticity_score: float
    temporal_score: float
    collaborative_score: float
    final_score: float
    confidence: float
    timestamp: float

class ContentRecommendationSystem(BaseAlgorithm):
    """Enhanced content recommendation system implementation"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.user_profiles: Dict[str, UserProfile] = {}
        self.content_vectors: Dict[str, ContentVector] = {}
        self.feedback_history: Dict[str, List[Dict[str, Any]]] = {}
        
        # Recommendation weights
        self.weights = {
            'relevance': 0.3,
            'engagement': 0.2,
            'authenticity': 0.2,
            'temporal': 0.15,
            'collaborative': 0.15
        }
    
        # Learning rate for weight adjustments
        self.learning_rate = 0.1
        self.min_confidence = 0.6
    
    def validate_input(self, data: Dict[str, Any]) -> bool:
        """Validate input data"""
        if 'feedback_id' in data:
            required_fields = {'feedback_id', 'algorithm_id', 'feedback_type', 'feedback_data'}
        else:
            required_fields = {'user_id', 'content_pool', 'user_history'}
        return all(field in data for field in required_fields)
    
    def add_user(self, user_id: str, interests: List[str], expertise_areas: List[str]) -> None:
        """Add or update user profile"""
        self.user_profiles[user_id] = UserProfile(
            user_id=user_id,
            interests=interests,
            expertise_areas=expertise_areas,
            engagement_history=[],
            feature_vector=self._create_user_vector(interests, expertise_areas)
        )
    
    def add_content(self, content_id: str, topics: List[str], complexity_level: float) -> None:
        """Add or update content vector"""
        self.content_vectors[content_id] = ContentVector(
            content_id=content_id,
            topics=topics,
            complexity_level=complexity_level,
            feature_vector=self._create_content_vector(topics, complexity_level)
        )
    
    def record_interaction(self, user_id: str, content_id: str, interaction_type: str, 
                         metrics: Dict[str, Any]) -> None:
        """Record user-content interaction"""
        if user_id in self.user_profiles:
            interaction = {
                'content_id': content_id,
                'type': interaction_type,
                'metrics': metrics,
                'timestamp': time.time()
            }
            self.user_profiles[user_id].engagement_history.append(interaction)
    
    def process(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process recommendation request"""
        if 'feedback_id' in data:
            return self._process_feedback(data)
        
        user_id = data['user_id']
        content_pool = data['content_pool']
        user_history = data.get('user_history', [])
        
        # Get user profile or create temporary one
        user_profile = self.user_profiles.get(user_id)
        if not user_profile:
            user_profile = self._create_temporary_profile(user_history)
        
        recommendations = []
        for content in content_pool:
            if content['id'] not in self.content_vectors:
                    self.add_content(
                    content['id'],
                    content.get('topics', []),
                    content.get('complexity_level', 0.5)
                )
            
            score = self._calculate_recommendation_score(user_profile, content)
            recommendations.append(score)
        
        # Sort by final score
        recommendations.sort(key=lambda x: x.final_score, reverse=True)
            
            return AlgorithmResponse(
            algorithm_id='content_recommendation_v2',
                timestamp=time.time(),
            results=[r.dict() for r in recommendations],
                metrics=self.get_metrics().dict()
            )
    
    def _process_feedback(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process feedback and optimize weights"""
        feedback_data = data['feedback_data']
        user_id = feedback_data.get('user_id', 'default')
        
        if user_id not in self.feedback_history:
            self.feedback_history[user_id] = []
        
        self.feedback_history[user_id].append(feedback_data)
        
        # Update weights based on feedback
        self._update_weights(user_id)
        
        return AlgorithmResponse(
            algorithm_id='content_recommendation_v2',
            timestamp=time.time(),
            results=[{'status': 'feedback_processed'}],
            metrics=self.get_metrics().dict()
        )
    
    def _create_user_vector(self, interests: List[str], expertise_areas: List[str]) -> List[float]:
        """Create user feature vector"""
        # Implementation details for creating user vector
        return [0.5] * 100  # Placeholder implementation
    
    def _create_content_vector(self, topics: List[str], complexity_level: float) -> List[float]:
        """Create content feature vector"""
        # Implementation details for creating content vector
        return [0.5] * 100  # Placeholder implementation
    
    def _calculate_recommendation_score(self, user: UserProfile, content: Dict[str, Any]) -> RecommendationScore:
        """Calculate comprehensive recommendation score"""
        content_id = content['id']
        content_vector = self.content_vectors[content_id]
        
        # Calculate individual scores
        relevance_score = self._calculate_relevance_score(user, content_vector)
        engagement_score = self._calculate_engagement_score(user, content)
        authenticity_score = content.get('authenticity_score', 0.5)
        temporal_score = self._calculate_temporal_score(content)
        collaborative_score = self._calculate_collaborative_score(user, content_id)
            
            # Calculate final score
            final_score = (
                self.weights['relevance'] * relevance_score +
                self.weights['engagement'] * engagement_score +
                self.weights['authenticity'] * authenticity_score +
            self.weights['temporal'] * temporal_score +
            self.weights['collaborative'] * collaborative_score
            )
            
        # Calculate confidence
        confidence = self._calculate_confidence(user, content)
        
        return RecommendationScore(
                content_id=content_id,
                relevance_score=relevance_score,
                engagement_score=engagement_score,
                authenticity_score=authenticity_score,
            temporal_score=temporal_score,
            collaborative_score=collaborative_score,
            final_score=final_score,
            confidence=confidence,
            timestamp=time.time()
        )
    
    def _calculate_relevance_score(self, user: UserProfile, content: ContentVector) -> float:
        """Calculate content relevance score"""
        if user.feature_vector and content.feature_vector:
            similarity = cosine_similarity(
                [user.feature_vector], 
                [content.feature_vector]
            )[0][0]
            return float(similarity)
        return 0.5
    
    def _calculate_engagement_score(self, user: UserProfile, content: Dict[str, Any]) -> float:
        """Calculate predicted engagement score"""
        # Implementation details for engagement score
        return 0.5  # Placeholder implementation
    
    def _calculate_temporal_score(self, content: Dict[str, Any]) -> float:
        """Calculate temporal relevance score"""
        # Implementation details for temporal score
        return 0.5  # Placeholder implementation
    
    def _calculate_collaborative_score(self, user: UserProfile, content_id: str) -> float:
        """Calculate collaborative filtering score"""
        similar_users = self._find_similar_users(user)
        if not similar_users:
            return 0.5
            
        total_score = 0.0
        total_weight = 0.0
        
        for similar_user, similarity in similar_users:
            # Check if similar user has interacted with content
            for interaction in similar_user.engagement_history:
                if interaction['content_id'] == content_id:
                    interaction_score = interaction['metrics'].get('engagement_score', 0.5)
                    total_score += similarity * interaction_score
                    total_weight += similarity
        
        if total_weight > 0:
            return total_score / total_weight
        return 0.5
    
    def _find_similar_users(self, user: UserProfile, k: int = 5) -> List[tuple]:
        """Find k most similar users"""
        if not user.feature_vector:
            return []
            
        similarities = []
        for other_id, other_user in self.user_profiles.items():
            if other_id != user.user_id and other_user.feature_vector:
                similarity = cosine_similarity(
                    [user.feature_vector], 
                    [other_user.feature_vector]
                )[0][0]
                similarities.append((other_user, float(similarity)))
        
        # Sort by similarity and return top k
        similarities.sort(key=lambda x: x[1], reverse=True)
        return similarities[:k]
    
    def _calculate_confidence(self, user: UserProfile, content: Dict[str, Any]) -> float:
        """Calculate confidence score for recommendation"""
        # Factors affecting confidence:
        # 1. User profile completeness
        # 2. Content vector quality
        # 3. Feedback history
        # 4. Similar user interactions
        
        base_confidence = 0.6
        
        # User profile factor
        if user.interests and user.expertise_areas:
            base_confidence += 0.1
        
        # Content vector factor
        content_vector = self.content_vectors.get(content['id'])
        if content_vector and content_vector.topics:
            base_confidence += 0.1
        
        # Feedback history factor
        user_id = user.user_id
        if user_id in self.feedback_history and self.feedback_history[user_id]:
            base_confidence += 0.1
        
        # Similar user interactions factor
        similar_users = self._find_similar_users(user)
        if similar_users:
            base_confidence += 0.1
        
        return min(0.95, base_confidence)
    
    def _update_weights(self, user_id: str) -> None:
        """Update recommendation weights based on feedback"""
        if user_id not in self.feedback_history:
            return
            
        feedback_history = self.feedback_history[user_id]
        if not feedback_history:
            return
            
        # Calculate weight adjustments based on feedback
        weight_adjustments = {k: 0.0 for k in self.weights}
        total_feedback = len(feedback_history)
        
        for feedback in feedback_history:
            metrics = feedback.get('metrics', {})
            
            # Adjust weights based on engagement metrics
            if 'engagement_score' in metrics:
                weight_adjustments['engagement'] += (
                    metrics['engagement_score'] - 0.5
                ) * self.learning_rate
            
            # Adjust weights based on relevance feedback
            if 'relevance_feedback' in metrics:
                weight_adjustments['relevance'] += (
                    metrics['relevance_feedback'] - 0.5
                ) * self.learning_rate
            
            # Adjust weights based on authenticity feedback
            if 'authenticity_feedback' in metrics:
                weight_adjustments['authenticity'] += (
                    metrics['authenticity_feedback'] - 0.5
                ) * self.learning_rate
        
        # Apply adjustments and normalize
        for key in self.weights:
            self.weights[key] = max(0.1, min(0.4, 
                self.weights[key] + weight_adjustments[key] / total_feedback
            ))
        
        # Normalize to sum to 1
        total_weight = sum(self.weights.values())
        self.weights = {k: v/total_weight for k, v in self.weights.items()} 