"""
Temporal Considerations Algorithm

This module implements temporal analysis for content relevance and decay
based on time-based factors and engagement patterns.
"""

import time
import math
from typing import Dict, Any, List, Optional
from datetime import datetime, timedelta

from pydantic import BaseModel

from .base import BaseAlgorithm, AlgorithmResponse

class TemporalScore(BaseModel):
    """Temporal score model"""
    content_id: str
    recency_score: float
    decay_rate: float
    time_sensitivity: float
    engagement_velocity: float
    timestamp: float

class TemporalConsiderations(BaseAlgorithm):
    """Temporal considerations implementation"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        
        # Default decay parameters
        self.base_decay_rate = 0.1  # Base rate of content decay
        self.engagement_boost = 0.2  # Boost from high engagement
        self.quality_boost = 0.15   # Boost from high quality
        
        # Time sensitivity thresholds (in hours)
        self.time_thresholds = {
            'breaking': 2,      # Breaking news/updates
            'current': 24,      # Current events
            'recent': 72,       # Recent discussions
            'relevant': 168,    # Still relevant (1 week)
            'evergreen': 720    # Long-term value (30 days)
        }
        
        # Content type weights
        self.content_weights = {
            'news': 1.2,
            'discussion': 1.0,
            'analysis': 0.8,
            'tutorial': 0.6,
            'reference': 0.4
        }
    
    def validate_input(self, data: Dict[str, Any]) -> bool:
        """Validate input data"""
        required_fields = {'content_data', 'reference_time'}
        return all(field in data for field in required_fields)
    
    def process(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process content and return temporal scores"""
        content_data = data['content_data']
        reference_time = data['reference_time']
        
        temporal_scores = []
        for content in content_data:
            score = self._calculate_temporal_score(content, reference_time)
            temporal_scores.append(score)
        
        return AlgorithmResponse(
            algorithm_id='temporal_considerations_v1',
            timestamp=time.time(),
            results=[score.dict() for score in temporal_scores],
            metrics=self.get_metrics().dict()
        )
    
    def _calculate_temporal_score(self, content: Dict[str, Any],
                                reference_time: float) -> TemporalScore:
        """Calculate temporal score for content"""
        content_id = content['content_id']
        content_timestamp = content['timestamp']
        content_type = content.get('type', 'discussion')
        
        # Calculate time-based metrics
        age_hours = (reference_time - content_timestamp) / 3600
        
        # Calculate recency score
        recency_score = self._calculate_recency(age_hours, content_type)
        
        # Calculate decay rate
        decay_rate = self._calculate_decay_rate(
            age_hours,
            content.get('engagement_metrics', {}),
            content.get('quality_metrics', {})
        )
        
        # Calculate time sensitivity
        time_sensitivity = self._calculate_time_sensitivity(
            content_type,
            content.get('metadata', {})
        )
        
        # Calculate engagement velocity
        engagement_velocity = self._calculate_engagement_velocity(
            content.get('engagement_metrics', {}),
            age_hours
        )
        
        return TemporalScore(
            content_id=content_id,
            recency_score=recency_score,
            decay_rate=decay_rate,
            time_sensitivity=time_sensitivity,
            engagement_velocity=engagement_velocity,
            timestamp=time.time()
        )
    
    def _calculate_recency(self, age_hours: float, content_type: str) -> float:
        """Calculate recency score based on age and content type"""
        # Get content type weight
        type_weight = self.content_weights.get(content_type, 1.0)
        
        # Calculate base recency score
        if age_hours < self.time_thresholds['breaking']:
            base_score = 1.0
        elif age_hours < self.time_thresholds['current']:
            base_score = 0.8
        elif age_hours < self.time_thresholds['recent']:
            base_score = 0.6
        elif age_hours < self.time_thresholds['relevant']:
            base_score = 0.4
        elif age_hours < self.time_thresholds['evergreen']:
            base_score = 0.2
        else:
            base_score = 0.1
        
        # Apply content type weight
        return min(1.0, base_score * type_weight)
    
    def _calculate_decay_rate(self, age_hours: float,
                            engagement_metrics: Dict[str, Any],
                            quality_metrics: Dict[str, Any]) -> float:
        """Calculate content decay rate"""
        # Start with base decay rate
        decay_rate = self.base_decay_rate
        
        # Adjust based on engagement
        engagement_level = engagement_metrics.get('level', 0.5)
        if engagement_level > 0.7:  # High engagement
            decay_rate -= self.engagement_boost
        elif engagement_level < 0.3:  # Low engagement
            decay_rate += self.engagement_boost
        
        # Adjust based on quality
        quality_score = quality_metrics.get('score', 0.5)
        if quality_score > 0.7:  # High quality
            decay_rate -= self.quality_boost
        elif quality_score < 0.3:  # Low quality
            decay_rate += self.quality_boost
        
        # Ensure decay rate stays within reasonable bounds
        return max(0.01, min(0.5, decay_rate))
    
    def _calculate_time_sensitivity(self, content_type: str,
                                 metadata: Dict[str, Any]) -> float:
        """Calculate time sensitivity of content"""
        # Base sensitivity by content type
        base_sensitivity = {
            'news': 0.9,
            'discussion': 0.7,
            'analysis': 0.5,
            'tutorial': 0.3,
            'reference': 0.1
        }.get(content_type, 0.5)
        
        # Adjust based on metadata tags
        tags = metadata.get('tags', [])
        if 'time-sensitive' in tags:
            base_sensitivity = min(1.0, base_sensitivity + 0.2)
        if 'evergreen' in tags:
            base_sensitivity = max(0.1, base_sensitivity - 0.2)
        
        return base_sensitivity
    
    def _calculate_engagement_velocity(self, engagement_metrics: Dict[str, Any],
                                    age_hours: float) -> float:
        """Calculate engagement velocity (rate of engagement over time)"""
        if age_hours == 0:
            return 0.0
            
        recent_views = engagement_metrics.get('recent_views', 0)
        total_views = engagement_metrics.get('total_views', 0)
        recent_interactions = engagement_metrics.get('recent_interactions', 0)
        total_interactions = engagement_metrics.get('total_interactions', 0)
        
        if total_views == 0 or total_interactions == 0:
            return 0.0
        
        # Calculate view velocity
        view_velocity = recent_views / max(1, total_views)
        
        # Calculate interaction velocity
        interaction_velocity = recent_interactions / max(1, total_interactions)
        
        # Combine velocities with time decay
        time_factor = math.exp(-age_hours / (24 * 7))  # 1-week half-life
        velocity = (0.7 * view_velocity + 0.3 * interaction_velocity) * time_factor
        
        return min(1.0, velocity) 