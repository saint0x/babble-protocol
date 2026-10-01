"""
Configuration settings for Babel Protocol algorithms.

This module contains all configuration settings and parameters used by the various
algorithms in the system. It provides a centralized place to manage algorithm behavior.
"""

from typing import Dict, Any
from pydantic_settings import BaseSettings
from pydantic import Field

class AlgorithmSettings(BaseSettings):
    """Base settings for all algorithms"""
    
    # API settings
    API_HOST: str = Field("localhost", env="API_HOST")
    API_PORT: int = Field(8000, env="API_PORT")
    DEBUG: bool = Field(True, env="DEBUG")
    
    # Database settings
    REDIS_URL: str = Field("redis://localhost:6379/0", env="REDIS_URL")
    POSTGRES_URL: str = Field("postgresql://localhost/babel", env="POSTGRES_URL")
    
    # Cache settings
    CACHE_TTL: int = Field(3600, env="CACHE_TTL")  # 1 hour default
    MAX_CACHE_SIZE: int = Field(10000, env="MAX_CACHE_SIZE")
    
    # Content Analysis settings
    MIN_CONTENT_LENGTH: int = Field(10, env="MIN_CONTENT_LENGTH")
    MAX_CONTENT_LENGTH: int = Field(50000, env="MAX_CONTENT_LENGTH")
    CONTENT_ANALYSIS_BATCH_SIZE: int = Field(100, env="CONTENT_ANALYSIS_BATCH_SIZE")
    
    # Recommendation settings
    MAX_RECOMMENDATIONS: int = Field(10, env="MAX_RECOMMENDATIONS")
    RECOMMENDATION_CACHE_TTL: int = Field(300, env="RECOMMENDATION_CACHE_TTL")  # 5 minutes
    SERENDIPITY_FACTOR: float = Field(0.2, env="SERENDIPITY_FACTOR")
    
    # Feedback Loop settings
    LEARNING_RATE: float = Field(0.1, env="LEARNING_RATE")
    ADJUSTMENT_THRESHOLD: float = Field(0.05, env="ADJUSTMENT_THRESHOLD")
    MIN_FEEDBACK_SAMPLES: int = Field(10, env="MIN_FEEDBACK_SAMPLES")
    
    # Community Moderation settings
    CONSENSUS_THRESHOLD: float = Field(0.7, env="CONSENSUS_THRESHOLD")
    MIN_MODERATOR_SCORE: float = Field(0.8, env="MIN_MODERATOR_SCORE")
    MODERATION_TIMEOUT: int = Field(86400, env="MODERATION_TIMEOUT")  # 24 hours
    
    # Optimization weights
    OPTIMIZATION_WEIGHTS: Dict[str, float] = {
        "user_satisfaction": 0.3,
        "content_quality": 0.2,
        "engagement_depth": 0.2,
        "community_health": 0.3
    }
    
    class Config:
        env_file = ".env"
        case_sensitive = True
        extra = "allow"

# Create global settings instance
settings = AlgorithmSettings()

# Algorithm-specific configurations
CONTENT_ANALYSIS_CONFIG = {
    "min_text_length": settings.MIN_CONTENT_LENGTH,
    "max_text_length": settings.MAX_CONTENT_LENGTH,
    "batch_size": settings.CONTENT_ANALYSIS_BATCH_SIZE,
    "nlp_models": {
        "sentiment": "vader_lexicon",
        "classification": "facebook/bart-large-mnli",
        "summarization": "facebook/bart-large-cnn"
    }
}

RECOMMENDATION_CONFIG = {
    "max_items": settings.MAX_RECOMMENDATIONS,
    "cache_ttl": settings.RECOMMENDATION_CACHE_TTL,
    "weights": {
        "relevance": 0.3,
        "engagement": 0.2,
        "authenticity": 0.4,
        "temporal": 0.1
    },
    "serendipity_factor": settings.SERENDIPITY_FACTOR
}

FEEDBACK_LOOP_CONFIG = {
    "learning_rate": settings.LEARNING_RATE,
    "adjustment_threshold": settings.ADJUSTMENT_THRESHOLD,
    "min_samples": settings.MIN_FEEDBACK_SAMPLES,
    "optimization_weights": settings.OPTIMIZATION_WEIGHTS
}

MODERATION_CONFIG = {
    "consensus_threshold": settings.CONSENSUS_THRESHOLD,
    "min_moderator_score": settings.MIN_MODERATOR_SCORE,
    "timeout": settings.MODERATION_TIMEOUT,
    "action_weights": {
        "flag": 0.3,
        "review": 0.5,
        "remove": 1.0
    }
}

def get_algorithm_config(algorithm_name: str) -> Dict[str, Any]:
    """Get configuration for a specific algorithm"""
    configs = {
        "content_analysis": CONTENT_ANALYSIS_CONFIG,
        "recommendation": RECOMMENDATION_CONFIG,
        "feedback_loop": FEEDBACK_LOOP_CONFIG,
        "moderation": MODERATION_CONFIG
    }
    return configs.get(algorithm_name, {}) 