"""
Recommendation Models

This module defines the data models used by the recommendation algorithms.
"""

from typing import Dict, Any, Optional
from pydantic import BaseModel

class RecommendationScore(BaseModel):
    """Model for recommendation scores"""
    content_id: str
    score: float
    relevance_score: float
    engagement_score: float
    authenticity_score: float
    temporal_score: float
    collaborative_score: Optional[float] = None
    timestamp: float
    metadata: Optional[Dict[str, Any]] = None 