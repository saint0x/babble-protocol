"""
Engagement Analytics Algorithm

This module implements engagement analysis to understand user interaction
patterns and content performance metrics.
"""

import time
from typing import Dict, Any, List, Optional
from datetime import datetime, timedelta
from collections import defaultdict

from pydantic import BaseModel

from .base import BaseAlgorithm, AlgorithmResponse

class EngagementMetrics(BaseModel):
    """Engagement metrics model"""
    user_id: str
    content_id: str
    session_duration: int
    scroll_depth: float
    time_of_day: str
    interaction_type: Optional[str] = 'view'  # Default to view
    timestamp: float

class EngagementSummary(BaseModel):
    """Engagement summary model"""
    total_sessions: int
    avg_session_duration: float
    avg_scroll_depth: float
    peak_hours: List[int]
    engagement_trend: Dict[str, float]
    user_segments: Dict[str, int]
    content_performance: Dict[str, Dict[str, float]]

class EngagementAnalytics(BaseAlgorithm):
    """Engagement analytics implementation"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.engagement_history: Dict[str, List[EngagementMetrics]] = {}
        
        # Engagement thresholds
        self.thresholds = {
            'high_engagement': 0.8,
            'medium_engagement': 0.5,
            'low_engagement': 0.2
        }
        
        # Time window for trend analysis (in seconds)
        self.trend_window = 24 * 60 * 60  # 24 hours
    
    def validate_input(self, data: Dict[str, Any]) -> bool:
        """Validate input data"""
        required_fields = {'engagement_data', 'time_window'}
        return all(field in data for field in required_fields)
    
    def process(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process engagement data and return analytics"""
        # Parse engagement data
        engagement_data = [
            EngagementMetrics(**entry) if isinstance(entry, dict) else entry
            for entry in data['engagement_data']
        ]
        time_window = data['time_window']
        
        # Update engagement history
        for entry in engagement_data:
            user_id = entry.user_id
            if user_id not in self.engagement_history:
                self.engagement_history[user_id] = []
            self.engagement_history[user_id].append(entry)
        
        # Calculate engagement metrics
        summary = self._analyze_engagement(engagement_data, time_window)
        
        return AlgorithmResponse(
            algorithm_id='engagement_analytics_v1',
            timestamp=time.time(),
            results=[summary.dict()],
            metrics=self.get_metrics().dict()
        )
    
    def _analyze_engagement(self, engagement_data: List[EngagementMetrics],
                          time_window: int) -> EngagementSummary:
        """Analyze engagement patterns and generate summary"""
        current_time = time.time()
        window_start = current_time - time_window
        
        # Filter data within time window
        recent_data = [
            entry for entry in engagement_data
            if entry.timestamp >= window_start
        ]
        
        if not recent_data:
            return EngagementSummary(
                total_sessions=0,
                avg_session_duration=0.0,
                avg_scroll_depth=0.0,
                peak_hours=[],
                engagement_trend={},
                user_segments={},
                content_performance={}
            )
        
        # Calculate basic metrics
        total_sessions = len(recent_data)
        avg_session_duration = sum(e.session_duration for e in recent_data) / total_sessions
        avg_scroll_depth = sum(e.scroll_depth for e in recent_data) / total_sessions
        
        # Analyze peak hours
        peak_hours = self._analyze_peak_hours(recent_data)
        
        # Calculate engagement trend
        engagement_trend = self._calculate_trend(recent_data, time_window)
        
        # Segment users
        user_segments = self._segment_users(recent_data)
        
        # Analyze content performance
        content_performance = self._analyze_content_performance(recent_data)
        
        return EngagementSummary(
            total_sessions=total_sessions,
            avg_session_duration=avg_session_duration,
            avg_scroll_depth=avg_scroll_depth,
            peak_hours=peak_hours,
            engagement_trend=engagement_trend,
            user_segments=user_segments,
            content_performance=content_performance
        )
    
    def _analyze_peak_hours(self, engagement_data: List[EngagementMetrics]) -> List[int]:
        """Analyze peak engagement hours"""
        hour_counts = defaultdict(int)
        
        for entry in engagement_data:
            hour = int(entry.time_of_day)
            hour_counts[hour] += 1
        
        # Find hours with above-average engagement
        avg_count = sum(hour_counts.values()) / max(1, len(hour_counts))
        peak_hours = [
            hour for hour, count in hour_counts.items()
            if count > avg_count
        ]
        
        return sorted(peak_hours)
    
    def _calculate_trend(self, engagement_data: List[EngagementMetrics],
                        time_window: int) -> Dict[str, float]:
        """Calculate engagement trends over time"""
        # Split time window into 6 periods
        period_length = time_window / 6
        periods = defaultdict(list)
        
        for entry in engagement_data:
            period_index = int((entry.timestamp % time_window) / period_length)
            period_key = f"period_{period_index + 1}"
            periods[period_key].append(entry)
        
        # Calculate average engagement per period
        trend = {}
        for period, entries in periods.items():
            if entries:
                avg_engagement = sum(e.scroll_depth * e.session_duration 
                                  for e in entries) / len(entries)
                trend[period] = avg_engagement
            else:
                trend[period] = 0.0
        
        return trend
    
    def _segment_users(self, engagement_data: List[EngagementMetrics]) -> Dict[str, int]:
        """Segment users based on engagement levels"""
        user_scores = defaultdict(list)
        
        for entry in engagement_data:
            # Calculate engagement score
            engagement_score = entry.scroll_depth * (entry.session_duration / 300)  # Normalize to 5 minutes
            user_scores[entry.user_id].append(engagement_score)
        
        segments = {
            'highly_engaged': 0,
            'moderately_engaged': 0,
            'low_engagement': 0
        }
        
        for user_id, scores in user_scores.items():
            avg_score = sum(scores) / len(scores)
            if avg_score >= self.thresholds['high_engagement']:
                segments['highly_engaged'] += 1
            elif avg_score >= self.thresholds['medium_engagement']:
                segments['moderately_engaged'] += 1
            else:
                segments['low_engagement'] += 1
        
        return segments
    
    def _analyze_content_performance(self, engagement_data: List[EngagementMetrics]) -> Dict[str, Dict[str, float]]:
        """Analyze content performance metrics"""
        content_metrics = defaultdict(lambda: {
            'total_sessions': 0,
            'avg_session_duration': 0.0,
            'avg_scroll_depth': 0.0,
            'engagement_score': 0.0
        })
        
        for entry in engagement_data:
            metrics = content_metrics[entry.content_id]
            metrics['total_sessions'] += 1
            metrics['avg_session_duration'] += entry.session_duration
            metrics['avg_scroll_depth'] += entry.scroll_depth
            metrics['engagement_score'] += (
                entry.scroll_depth * (entry.session_duration / 300)
            )
        
        # Calculate averages
        for content_id, metrics in content_metrics.items():
            total = metrics['total_sessions']
            if total > 0:
                metrics['avg_session_duration'] /= total
                metrics['avg_scroll_depth'] /= total
                metrics['engagement_score'] /= total
        
        return dict(content_metrics) 