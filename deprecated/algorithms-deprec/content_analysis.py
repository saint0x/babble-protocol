"""
Content Analysis Algorithm

This module implements content analysis using NLP techniques to extract
features, topics, and other relevant information from content.
"""

import time
import re
from typing import Dict, Any, List, Optional
from datetime import datetime

import nltk
from nltk.tokenize import word_tokenize
from nltk.corpus import stopwords
from nltk.sentiment import SentimentIntensityAnalyzer
from pydantic import BaseModel

from .base import BaseAlgorithm, AlgorithmResponse

# Download required NLTK data
nltk.download('stopwords', quiet=True)
nltk.download('vader_lexicon', quiet=True)

class ContentAnalysisResult(BaseModel):
    """Content analysis result model"""
    content_id: str
    text: str
    metadata: Dict[str, Any]
    analysis: Dict[str, Any]
    context_references: List[str]
    evidence_count: int

class ContentAnalysis(BaseAlgorithm):
    """Content analysis implementation"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.sentiment_analyzer = SentimentIntensityAnalyzer()
        self.stop_words = set(stopwords.words('english'))
        
        # Topic categories for classification
        self.topic_categories = [
            "technology", "science", "politics", "economics",
            "culture", "health", "environment", "education",
            "society", "philosophy"
        ]
        
        # Evidence quality markers
        self.evidence_markers = [
            "according to", "research shows", "study finds",
            "evidence suggests", "data indicates", "experts say",
            "analysis reveals", "investigation shows"
        ]
    
    def validate_input(self, data: Dict[str, Any]) -> bool:
        """Validate input data"""
        required_fields = {'content_id', 'text'}
        return all(field in data for field in required_fields)
    
    def process(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process content and return analysis results"""
        # Check cache first
        cache_key = f"analysis:{data['content_id']}"
        cached_result = self.get_cache(cache_key)
        if cached_result:
            return AlgorithmResponse.parse_raw(cached_result)
        
        # Initialize result
        result = ContentAnalysisResult(
            content_id=data['content_id'],
            text=data['text'],
            metadata=data.get('metadata', {}),
            analysis={},
            context_references=[],
            evidence_count=0
        )
        
        try:
            # Basic text analysis
            text_properties = self._analyze_text_properties(data['text'])
            result.analysis['text_properties'] = text_properties
            
            # Semantic analysis
            semantics = self._analyze_semantics(data['text'])
            result.analysis['semantics'] = semantics
            
            # Topic classification
            topics = self._classify_topics(data['text'])
            result.analysis['topics'] = topics
            
            # Evidence analysis
            evidence = self._analyze_evidence(data['text'])
            result.analysis['evidence'] = evidence
            result.evidence_count = evidence['count']
            
            # Context references
            context = self._extract_context(data['text'])
            result.context_references = context['references']
            result.analysis['context'] = context
            
            # Generate summary
            if len(data['text']) > 100:
                summary = self._generate_summary(data['text'])
                result.analysis['summary'] = summary
            
        except Exception as e:
            self.log_error(e, {'content_id': data['content_id']})
            raise
        
        # Create response
        response = AlgorithmResponse(
            algorithm_id='content_analysis_v1',
            timestamp=time.time(),
            results=[result.dict()],
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
    
    def _analyze_text_properties(self, text: str) -> Dict[str, Any]:
        """Analyze basic text properties"""
        # Simple sentence splitting on punctuation
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        words = self._tokenize(text)
        
        return {
            'sentence_count': len(sentences),
            'word_count': len(words),
            'avg_sentence_length': len(words) / max(1, len(sentences)),
            'unique_words': len(set(words)),
            'vocabulary_richness': len(set(words)) / max(1, len(words))
        }
    
    def _analyze_semantics(self, text: str) -> Dict[str, Any]:
        """Analyze semantic properties of text"""
        # Sentiment analysis
        sentiment = self.sentiment_analyzer.polarity_scores(text)
        
        # Complexity analysis
        words = self._tokenize(text)
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        avg_word_length = sum(len(w) for w in words) / max(1, len(words))
        avg_sentence_length = len(words) / max(1, len(sentences))
        unique_ratio = len(set(words)) / max(1, len(words))
        complexity_score = (0.3 * avg_word_length + 0.4 * avg_sentence_length + 0.3 * unique_ratio) / 10
        
        return {
            'sentiment': sentiment,
            'complexity_score': complexity_score,
            'entities': []  # Placeholder for entity extraction
        }
    
    def _classify_topics(self, text: str) -> Dict[str, float]:
        """Classify text into predefined topics"""
        # Simple keyword-based classification for now
        words = set(self._tokenize(text))
        topics = {}
        
        # Technology keywords
        tech_words = {'ai', 'technology', 'software', 'digital', 'computer', 'blockchain', 'crypto'}
        topics['technology'] = len(words & tech_words) / max(1, len(words))
        
        # Science keywords
        science_words = {'research', 'study', 'analysis', 'data', 'scientific', 'experiment'}
        topics['science'] = len(words & science_words) / max(1, len(words))
        
        # Normalize scores
        total = sum(topics.values())
        if total > 0:
            topics = {k: v/total for k, v in topics.items()}
        
        return topics
    
    def _analyze_evidence(self, text: str) -> Dict[str, Any]:
        """Analyze evidence and citation patterns"""
        text_lower = text.lower()
        evidence_count = sum(1 for marker in self.evidence_markers if marker in text_lower)
        
        return {
            'count': evidence_count,
            'strength_score': min(1.0, evidence_count / 5),  # Normalize to 0-1
            'markers_found': [m for m in self.evidence_markers if m in text_lower]
        }
    
    def _extract_context(self, text: str) -> Dict[str, Any]:
        """Extract context references and citations"""
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        references = []
        
        for sentence in sentences:
            if any(marker in sentence.lower() for marker in self.evidence_markers):
                references.append(sentence)
        
        return {
            'references': references,
            'reference_count': len(references)
        }
    
    def _generate_summary(self, text: str) -> Dict[str, Any]:
        """Generate text summary"""
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        
        # Simple extractive summary - first sentence and any evidence sentences
        summary_sentences = [sentences[0]]
        for sentence in sentences[1:]:
            if any(marker in sentence.lower() for marker in self.evidence_markers):
                summary_sentences.append(sentence)
        
        summary_text = '. '.join(summary_sentences[:3]) + '.'  # Limit to 3 sentences
        
        return {
            'summary_text': summary_text,
            'length': len(self._tokenize(summary_text))
        } 