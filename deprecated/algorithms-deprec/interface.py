"""
Algorithm Interface for Babel Protocol

This module provides a unified interface for interacting with all algorithm implementations.
It handles algorithm initialization, execution, and result aggregation.
"""

from typing import Dict, List, Any, Optional
from datetime import datetime
import json
import time

from .content_analysis import ContentAnalysis
from .recommendation import ContentRecommendationSystem
from .config import settings

class AlgorithmInterface:
    """Interface for managing and executing algorithms"""
    
    def __init__(self):
        """Initialize algorithm instances"""
        self.content_analysis = ContentAnalysis(
            redis_url=settings.REDIS_URL,
            postgres_url=settings.POSTGRES_URL,
            cache_ttl=settings.CACHE_TTL
        )
        
        self.recommendation = ContentRecommendationSystem(
            redis_url=settings.REDIS_URL,
            postgres_url=settings.POSTGRES_URL,
            cache_ttl=settings.RECOMMENDATION_CACHE_TTL
        )
        
        # Initialize content store
        self._initialize_content_store()
    
    def _initialize_content_store(self):
        """Initialize recommendation system with test content"""
        test_content = {
            "technical_post": {
                "content_id": "post_001",
                "text": "Zero-knowledge proofs are revolutionizing DeFi privacy...",
                "timestamp": time.time() - 3600,  # 1 hour ago
                "authenticity_score": 0.9
            },
            "philosophical_post": {
                "content_id": "post_002", 
                "text": "As AI systems increasingly make decisions affecting human lives...",
                "timestamp": time.time() - 7200,  # 2 hours ago
                "authenticity_score": 0.85
            },
            "market_analysis": {
                "content_id": "post_003",
                "text": "Analysis of DeFi market trends in Q4 2023...",
                "timestamp": time.time() - 1800,  # 30 mins ago
                "authenticity_score": 0.95
            }
        }
        
        for content in test_content.values():
            self.recommendation.add_content(
                content_id=content["content_id"],
                text=content["text"],
                timestamp=content["timestamp"],
                authenticity_score=content["authenticity_score"]
            )
    
    def process_content(self, content: Dict[str, Any]) -> Dict[str, Any]:
        """Process new content through content analysis"""
        try:
            analysis_result = self.content_analysis.execute(content)
            
            # Update recommendation system
            self.recommendation.add_content(
                content_id=content["content_id"],
                text=content["text"],
                timestamp=time.time(),
                authenticity_score=analysis_result.analysis.get("evidence", {}).get("strength_score", 0.5)
            )
            
            return {
                "content_id": content["content_id"],
                "analysis": analysis_result.dict(),
                "timestamp": datetime.now().isoformat()
            }
        except Exception as e:
            return {
                "error": str(e),
                "content_id": content.get("content_id"),
                "timestamp": datetime.now().isoformat()
            }
    
    def get_recommendations(self, user_id: str) -> List[Dict[str, Any]]:
        """Get content recommendations for user"""
        try:
            recommendations = self.recommendation.execute({"user_id": user_id})
            return [rec.dict() for rec in recommendations]
        except Exception as e:
            return [{"error": str(e), "user_id": user_id}]
    
    def record_feedback(
        self,
        content_id: str,
        user_id: str,
        feedback_type: str,
        feedback_value: float
    ) -> Dict[str, Any]:
        """Record user feedback and update recommendation system"""
        try:
            # Update engagement score based on feedback
            self.recommendation.update_engagement(
                content_id=content_id,
                engagement_score=feedback_value
            )
            
            return {
                "status": "success",
                "content_id": content_id,
                "user_id": user_id,
                "feedback_type": feedback_type,
                "feedback_value": feedback_value,
                "timestamp": datetime.now().isoformat()
            }
        except Exception as e:
            return {
                "status": "error",
                "error": str(e),
                "content_id": content_id,
                "user_id": user_id
            }
    
    def get_algorithm_status(self) -> Dict[str, Any]:
        """Get status of all algorithms"""
        return {
            "content_analysis": {
                "metrics": self.content_analysis.get_metrics().dict(),
                "status": "active"
            },
            "recommendation": {
                "metrics": self.recommendation.get_metrics().dict(),
                "status": "active"
            },
            "timestamp": datetime.now().isoformat()
        }

# Create global algorithm interface instance
algorithm_interface = AlgorithmInterface() 