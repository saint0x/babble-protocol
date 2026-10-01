"""
Community Moderation Algorithm

This module implements community-driven content moderation using
sentiment analysis, spam detection, and content quality assessment.
"""

import time
import re
from typing import Dict, Any, List, Optional
from datetime import datetime

from pydantic import BaseModel
import nltk
from nltk.corpus import stopwords
from nltk.sentiment import SentimentIntensityAnalyzer

from .base import BaseAlgorithm, AlgorithmResponse

# Download required NLTK data
nltk.download('stopwords', quiet=True)
nltk.download('vader_lexicon', quiet=True)

class ModerationResult(BaseModel):
    """Moderation result model"""
    content_id: str
    flags: List[str]
    spam_score: float
    quality_score: float
    sentiment_score: float
    action_needed: bool
    timestamp: float

class CommunityModerationSystem(BaseAlgorithm):
    """Community moderation system implementation"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.sentiment_analyzer = SentimentIntensityAnalyzer()
        self.stop_words = set(stopwords.words('english'))
        
        # Spam detection patterns
        self.spam_patterns = [
            r'\b(buy|sell|discount|offer|price|deal|free|win|click)\b',
            r'https?://\S+',
            r'\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b',
            r'\b(urgent|limited time|act now|don\'t wait)\b',
            r'[!?]{2,}',
            r'[A-Z]{4,}'
        ]
        
        # Quality assessment factors
        self.quality_factors = {
            'length': 0.3,
            'complexity': 0.2,
            'formatting': 0.2,
            'engagement': 0.3
        }
        
        # Action thresholds
        self.spam_threshold = 0.7
        self.quality_threshold = 0.3
        self.sentiment_threshold = -0.6
    
    def validate_input(self, data: Dict[str, Any]) -> bool:
        """Validate input data"""
        required_fields = {'content_id', 'text'}
        return all(field in data for field in required_fields)
    
    def process(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process content and return moderation results"""
        # Check cache first
        cache_key = f"moderation:{data['content_id']}"
        cached_result = self.get_cache(cache_key)
        if cached_result:
            return AlgorithmResponse.parse_raw(cached_result)
        
        text = data['text']
        analysis = data.get('analysis', {})
        metadata = data.get('metadata', {})
        context = data.get('context', {})
        
        # Calculate scores
        spam_score = self.check_spam_score(data)
        quality_score = self._assess_quality(text, analysis, metadata)
        sentiment_score = self._analyze_sentiment(text)
        
        # Determine flags and action
        flags = []
        action = 'allow'
        
        # Check for hate speech
        if self._contains_hate_speech(text):
            flags.append('hate_speech')
            action = 'remove'
        
        # Check for spam
        if spam_score > self.spam_threshold:
            flags.append('spam')
            if spam_score > 0.8:
                action = 'remove'
            else:
                action = 'warn'
        
        # Check for misinformation
        if self._check_misinformation(text, context):
            flags.append('misinformation')
            action = 'flag'
        
        # Check for coordinated behavior
        if self._check_coordinated_behavior(context):
            flags.append('coordinated')
            action = 'remove'
        
        # Check for borderline content
        if quality_score < self.quality_threshold or sentiment_score < self.sentiment_threshold:
            flags.append('low_quality')
            if not flags:  # Only warn if no other flags
                action = 'warn'
        
        result = {
            'content_id': data['content_id'],
            'flags': flags,
            'action': action,
            'scores': {
                'spam': spam_score,
                'quality': quality_score,
                'sentiment': sentiment_score
            },
            'timestamp': time.time()
        }
        
        response = AlgorithmResponse(
            algorithm_id='community_moderation_v1',
            timestamp=time.time(),
            results=[result],
            metrics=self.get_metrics().dict()
        )
        
        # Cache results
        self.set_cache(cache_key, response.json())
        
        return response
    
    def _tokenize(self, text: str) -> List[str]:
        """Simple word tokenization"""
        # Split on whitespace and punctuation
        words = re.findall(r'\b\w+\b', text.lower())
        return [w for w in words if w not in self.stop_words]
    
    def check_spam_score(self, content: Dict[str, Any]) -> float:
        """Calculate spam score for content"""
        try:
            text = content.get('text', '').lower()
            if not isinstance(text, str):
                return 0.0
            
            # Check for common spam indicators
            spam_indicators = {
                'promotional': [
                    'buy now', 'limited time', 'act now', 'special offer',
                    'discount', 'free', 'click here', 'sign up now',
                    'best price', 'money back', 'guarantee'
                ],
                'urgency': [
                    'urgent', 'hurry', 'limited', 'expires', 'today only',
                    'last chance', 'don\'t miss', 'act fast'
                ],
                'excessive': [
                    '!!!', '???', '$$$', 'www.', 'http://', 'https://',
                    '@gmail', '@yahoo', '@hotmail'
                ]
            }
            
            # Calculate category scores
            scores = {}
            for category, indicators in spam_indicators.items():
                matches = sum(1 for ind in indicators if ind in text)
                scores[category] = min(1.0, matches / 3)  # Cap at 1.0
            
            # Check for repetitive patterns
            words = text.split()
            if len(words) > 0:
                unique_ratio = len(set(words)) / len(words)
                repetition_score = 1.0 - unique_ratio
            else:
                repetition_score = 0.0
            
            # Check for excessive capitalization
            if len(text) > 0:
                caps_ratio = sum(1 for c in text if c.isupper()) / len(text)
                caps_score = 1.0 if caps_ratio > 0.3 else caps_ratio
            else:
                caps_score = 0.0
            
            # Calculate final spam score with weighted components
            spam_score = (
                0.3 * scores.get('promotional', 0.0) +
                0.2 * scores.get('urgency', 0.0) +
                0.2 * scores.get('excessive', 0.0) +
                0.15 * repetition_score +
                0.15 * caps_score
            )
            
            return min(1.0, spam_score)
        except Exception as e:
            self.logger.error(f"Error in spam detection: {str(e)}")
            return 0.0
    
    def _assess_quality(self, text: str, analysis: Dict[str, Any],
                       metadata: Dict[str, Any]) -> float:
        """Assess content quality"""
        scores = {}
        
        # Length score
        text_length = len(text.split())
        scores['length'] = min(1.0, text_length / 500)  # Normalize to 500 words
        
        # Complexity score
        complexity = analysis.get('semantics', {}).get('complexity_score', 0.5)
        scores['complexity'] = complexity
        
        # Formatting score
        formatting_score = self._assess_formatting(text)
        scores['formatting'] = formatting_score
        
        # Engagement potential score
        engagement_score = self._assess_engagement_potential(text, analysis)
        scores['engagement'] = engagement_score
        
        # Calculate weighted average
        final_score = sum(
            self.quality_factors[factor] * scores[factor]
            for factor in self.quality_factors
        )
        
        return final_score
    
    def _assess_formatting(self, text: str) -> float:
        """Assess text formatting quality"""
        score = 1.0
        
        # Check for proper capitalization
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        if sentences:
            properly_capitalized = sum(
                1 for s in sentences if s[0].isupper()
            ) / len(sentences)
            score *= 0.5 + 0.5 * properly_capitalized
        
        # Check for paragraph breaks
        if len(text) > 200 and text.count('\n\n') == 0:
            score *= 0.8
        
        # Check for excessive caps
        caps_ratio = sum(1 for c in text if c.isupper()) / max(1, len(text))
        if caps_ratio > 0.3:
            score *= 0.7
        
        return score
    
    def _assess_engagement_potential(self, text: str, analysis: Dict[str, Any]) -> float:
        """Assess potential for user engagement"""
        score = 0.0
        
        # Check for questions (encourages discussion)
        question_count = text.count('?')
        score += min(0.3, question_count * 0.1)
        
        # Check for evidence/citations
        evidence_count = analysis.get('evidence', {}).get('count', 0)
        score += min(0.3, evidence_count * 0.1)
        
        # Check for balanced sentiment
        sentiment = analysis.get('semantics', {}).get('sentiment', {})
        if sentiment:
            # Prefer balanced or slightly positive content
            compound = sentiment.get('compound', 0)
            score += 0.4 * (1.0 - abs(compound - 0.2))
        
        return score
    
    def _analyze_sentiment(self, text: str) -> float:
        """Analyze text sentiment"""
        sentiment = self.sentiment_analyzer.polarity_scores(text)
        return sentiment['compound']
    
    def _contains_hate_speech(self, text: str) -> bool:
        """Check for hate speech indicators"""
        # Convert to lowercase for matching
        text_lower = text.lower()
        
        # Check for slurs or discriminatory language
        if '[ethnic slur]' in text_lower or '[demographic]' in text_lower:
            return True
        
        # Check sentiment for extreme negativity
        sentiment = self.sentiment_analyzer.polarity_scores(text)
        if sentiment['compound'] < -0.7:  # Very negative
            # Look for personal attacks or discriminatory patterns
            attack_patterns = [
                r'\b(hate|stupid|idiot|dumb|incompetent)\b.*\b(you|they|them|those)\b',
                r'\b(all|every|those)\b.*\b(people|users|members)\b.*\b(are|should)\b',
                r'\b(get rid of|remove|ban)\b.*\b(all|every|those)\b'
            ]
            for pattern in attack_patterns:
                if re.search(pattern, text_lower):
                    return True
        
        return False
    
    def _check_misinformation(self, text: str, context: Dict[str, Any]) -> bool:
        """Check for potential misinformation"""
        # Check for unverified claims patterns
        unverified_patterns = [
            r'\b(insider|source|leaked|breaking|urgent)\b',
            r'\b(all|every|none|never|always)\b.*\b(will|are|have)\b',
            r'!(dev|team|protocol).*!(run|steal|hack)',
            r'\b(million|billion|trillion)\b.*\b(stolen|lost|hacked)\b'
        ]
        
        matches = 0
        for pattern in unverified_patterns:
            if re.search(pattern, text.lower()):
                matches += 1
        
        # Consider user reports
        if context.get('user_reports', 0) > 5:
            matches += 1
        
        return matches >= 2
    
    def _check_coordinated_behavior(self, context: Dict[str, Any]) -> bool:
        """Check for signs of coordinated behavior"""
        pattern = context.get('pattern', {})
        
        # Check for multiple similar posts in short time
        if pattern.get('similar_posts', 0) > 10:
            time_window = pattern.get('time_window', '')
            if time_window.endswith('h') and int(time_window[:-1]) <= 1:
                return True
        
        # Check for coordinated accounts
        if pattern.get('coordinated_accounts', False):
            return True
        
        return False 