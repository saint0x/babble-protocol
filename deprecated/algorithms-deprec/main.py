"""
Main FastAPI Application for Babel Protocol Algorithms

This module provides the main FastAPI application and API endpoints for the algorithm service.
It serves as the entry point for all algorithm-related HTTP requests.
"""

from typing import Dict, List, Optional
from fastapi import FastAPI, HTTPException, Depends
from pydantic import BaseModel, Field

from .interface import algorithm_interface
from .config import settings

# Create FastAPI app
app = FastAPI(
    title="Babel Protocol Algorithm Service",
    description="API for content analysis, recommendations, and algorithm optimization",
    version="1.0.0"
)

# Request/Response Models
class ContentRequest(BaseModel):
    """Request model for content processing"""
    content_id: str = Field(..., description="Unique identifier for the content")
    text: str = Field(..., description="Content text to process")
    metadata: Optional[Dict] = Field(default=None, description="Optional metadata about the content")

class ContentResponse(BaseModel):
    """Response model for processed content"""
    content_id: str
    analysis: Dict
    recommendations: List[Dict]
    status: str

class FeedbackRequest(BaseModel):
    """Request model for user feedback"""
    user_id: str = Field(..., description="User providing the feedback")
    content_id: str = Field(..., description="Content being rated")
    feedback_type: str = Field(..., description="Type of feedback (engagement, satisfaction, issue, suggestion)")
    feedback_data: Dict = Field(..., description="Detailed feedback information")

class FeedbackResponse(BaseModel):
    """Response model for processed feedback"""
    feedback_id: str
    status: str
    optimization_status: Dict

class RecommendationRequest(BaseModel):
    """Request model for recommendations"""
    user_id: str = Field(..., description="User to get recommendations for")
    count: Optional[int] = Field(default=None, description="Number of recommendations to return")

# Health check endpoint
@app.get("/health")
async def health_check():
    """Check service health"""
    return {
        "status": "healthy",
        "version": "1.0.0",
        "algorithms": list(algorithm_interface.get_algorithm_status().keys())
    }

# API Endpoints
@app.post("/content/process", response_model=ContentResponse)
async def process_content(request: ContentRequest):
    """Process new content through all algorithms"""
    try:
        result = await algorithm_interface.process_content(
            content_id=request.content_id,
            text=request.text,
            metadata=request.metadata
        )
        return result
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/feedback", response_model=FeedbackResponse)
async def record_feedback(request: FeedbackRequest):
    """Record and process user feedback"""
    try:
        result = await algorithm_interface.record_user_feedback(
            user_id=request.user_id,
            content_id=request.content_id,
            feedback_type=request.feedback_type,
            feedback_data=request.feedback_data
        )
        return result
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.post("/recommendations", response_model=List[Dict])
async def get_recommendations(request: RecommendationRequest):
    """Get personalized content recommendations"""
    try:
        recommendations = await algorithm_interface.get_recommendations(
            user_id=request.user_id,
            count=request.count
        )
        return recommendations
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/content/{content_id}/related", response_model=List[Dict])
async def get_related_content(content_id: str, count: Optional[int] = 5):
    """Get content related to a specific piece of content"""
    try:
        related = await algorithm_interface.get_related_content(
            content_id=content_id,
            count=count
        )
        return related
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

@app.get("/status")
async def get_status():
    """Get current status of all algorithms"""
    try:
        return algorithm_interface.get_algorithm_status()
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

# Error handlers
@app.exception_handler(Exception)
async def general_exception_handler(request, exc):
    """Handle all uncaught exceptions"""
    return {
        "status": "error",
        "message": str(exc),
        "type": type(exc).__name__
    } 