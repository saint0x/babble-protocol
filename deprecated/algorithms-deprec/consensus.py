"""
Source of Truth Consensus Algorithm

This module implements consensus mechanisms for determining source reliability
and content authenticity through community validation.
"""

import time
from typing import Dict, Any, List, Optional
from datetime import datetime
import re
from enum import Enum

from pydantic import BaseModel
import nltk
from nltk.tokenize import word_tokenize
from nltk.corpus import stopwords

from .base import BaseAlgorithm, AlgorithmResponse

class ConsensusState(str, Enum):
    """Consensus states for content validation"""
    EMERGING = "emerging"       # > 40% agreement
    PROVISIONAL = "provisional" # > 60% agreement
    ESTABLISHED = "established" # > 80% agreement
    CONTESTED = "contested"     # Was established, now < 60%
    REVOKED = "revoked"        # Was established, now < 40%
    INSUFFICIENT = "insufficient" # < 40% agreement

class ConsensusResult(BaseModel):
    """Consensus result model"""
    content_id: str
    sources: List[Dict[str, Any]]
    consensus_score: float
    reliability_score: float
    validation_count: int
    consensus_state: ConsensusState
    temporal_weight: float
    user_contributions: Dict[str, float]
    timestamp: float

class SourceOfTruthConsensus(BaseAlgorithm):
    """Source of truth consensus implementation"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.stop_words = set(stopwords.words('english'))
        
        # Source reliability weights
        self.source_weights = {
            'official_docs': 1.0,    # Official documentation
            'research_paper': 0.9,   # Academic research
            'technical_blog': 0.8,   # Technical blog posts
            'community_wiki': 0.7,   # Community wikis
            'forum_post': 0.6,       # Forum discussions
            'social_media': 0.4      # Social media posts
        }
        
        # Validation thresholds
        self.validation_thresholds = {
            'high': 0.8,
            'medium': 0.6,
            'low': 0.4
        }
        
        # Content similarity threshold
        self.similarity_threshold = 0.7
        
        # Temporal decay settings
        self.half_life_days = 7.0  # Time in days for 50% weight reduction
    
    def validate_input(self, data: Dict[str, Any]) -> bool:
        """Validate input data"""
        required_fields = {'content_id', 'sources'}
        return all(field in data for field in required_fields)
    
    def _calculate_temporal_weight(self, timestamp: float) -> float:
        """Calculate weight based on time decay"""
        age = time.time() - timestamp
        half_life = self.half_life_days * 24 * 3600  # Convert days to seconds
        return 2 ** (-age / half_life)
    
    def _determine_consensus_state(self, 
                                 current_score: float, 
                                 previous_score: Optional[float] = None) -> ConsensusState:
        """Determine the consensus state based on current and previous scores"""
        if previous_score is None:
            if current_score >= self.validation_thresholds['high']:
                return ConsensusState.ESTABLISHED
            elif current_score >= self.validation_thresholds['medium']:
                return ConsensusState.PROVISIONAL
            elif current_score >= self.validation_thresholds['low']:
                return ConsensusState.EMERGING
            return ConsensusState.INSUFFICIENT
        
        # Handle state transitions
        if previous_score >= self.validation_thresholds['high']:
            if current_score < self.validation_thresholds['medium']:
                return ConsensusState.CONTESTED
            elif current_score < self.validation_thresholds['low']:
                return ConsensusState.REVOKED
            return ConsensusState.ESTABLISHED
        
        return self._determine_consensus_state(current_score)
    
    def _calculate_user_contribution(self, 
                                   user_vote_timestamp: float,
                                   final_consensus: float,
                                   user_vote_value: float) -> float:
        """Calculate user's contribution to consensus"""
        temporal_weight = self._calculate_temporal_weight(user_vote_timestamp)
        agreement_with_consensus = 1 - abs(final_consensus - user_vote_value)
        return temporal_weight * agreement_with_consensus
    
    def process(self, data: Dict[str, Any]) -> AlgorithmResponse:
        """Process sources and return consensus results"""
        try:
            # Validate and extract content_id
            content_id = str(data.get('content_id', ''))
            if not content_id:
                raise ValueError("Missing or invalid content_id")
            
            # Validate and extract sources
            sources = data.get('sources', [])
            if not isinstance(sources, list):
                raise ValueError("Sources must be a list")
            
            # Calculate consensus metrics
            consensus_score = self._calculate_consensus(sources)
            reliability_score = self._calculate_reliability(sources)
            validation_count = len(sources)
            
            # Calculate temporal weight
            current_time = time.time()
            temporal_weight = self._calculate_temporal_weight(
                min(s.get('timestamp', current_time) for s in sources)
            )
            
            # Calculate user contributions
            user_contributions = {}
            for source in sources:
                if 'user_id' in source and 'vote_value' in source:
                    contribution = self._calculate_user_contribution(
                        source.get('timestamp', current_time),
                        consensus_score,
                        source['vote_value']
                    )
                    user_contributions[source['user_id']] = contribution
            
            # Determine consensus state
            previous_score = data.get('previous_consensus_score')
            consensus_state = self._determine_consensus_state(
                consensus_score, 
                previous_score
            )
            
            result = ConsensusResult(
                content_id=content_id,
                sources=sources,
                consensus_score=consensus_score,
                reliability_score=reliability_score,
                validation_count=validation_count,
                consensus_state=consensus_state,
                temporal_weight=temporal_weight,
                user_contributions=user_contributions,
                timestamp=current_time
            )
            
            return AlgorithmResponse(
                algorithm_id='source_consensus_v1',
                timestamp=current_time,
                results=[result.dict()],
                metrics=self.get_metrics().dict()
            )
        except Exception as e:
            # Return error response
            return AlgorithmResponse(
                algorithm_id='source_consensus_v1',
                timestamp=time.time(),
                results=[{
                    'error': str(e),
                    'content_id': data.get('content_id', 'unknown')
                }],
                metrics=self.get_metrics().dict()
            )
    
    def _calculate_consensus(self, sources: List[Dict[str, Any]]) -> float:
        """Calculate consensus score based on source agreement"""
        if not sources:
            return 0.0
        
        # Extract key information from each source
        source_info = []
        for source in sources:
            content = source['content']
            text = content.get('text', '')
            
            # Extract key terms and facts
            key_terms = self._extract_key_terms(text)
            facts = self._extract_facts(text)
            
            source_info.append({
                'key_terms': key_terms,
                'facts': facts,
                'weight': self.source_weights.get(source['source'], 0.5)
            })
        
        # Calculate agreement scores
        term_agreement = self._calculate_term_agreement(
            [info['key_terms'] for info in source_info]
        )
        
        fact_agreement = self._calculate_fact_agreement(
            [info['facts'] for info in source_info]
        )
        
        # Weight the scores by source reliability
        weights = [info['weight'] for info in source_info]
        weighted_score = (
            0.4 * term_agreement +
            0.6 * fact_agreement
        ) * sum(weights) / len(weights)
        
        return min(1.0, weighted_score)
    
    def _calculate_reliability(self, sources: List[Dict[str, Any]]) -> float:
        """Calculate overall reliability score of sources"""
        if not sources:
            return 0.0
        
        reliability_scores = []
        for source in sources:
            # Get base reliability from source type
            base_reliability = self.source_weights.get(source['source'], 0.5)
            
            # Adjust based on content quality
            content = source['content']
            quality_score = content.get('analysis', {}).get('quality_score', 0.5)
            
            # Adjust based on evidence strength
            evidence_score = content.get('analysis', {}).get('evidence', {}).get('strength_score', 0.5)
            
            # Calculate final reliability
            reliability = (
                0.4 * base_reliability +
                0.3 * quality_score +
                0.3 * evidence_score
            )
            reliability_scores.append(reliability)
        
        # Return weighted average
        return sum(reliability_scores) / len(reliability_scores)
    
    def _extract_key_terms(self, text: str) -> List[str]:
        """Extract key terms from text"""
        # Split on whitespace and punctuation
        words = re.findall(r'\b\w+\b', text.lower())
        words = [w for w in words if w not in self.stop_words and len(w) > 2]
        
        # Count term frequencies
        term_freq = {}
        for word in words:
            term_freq[word] = term_freq.get(word, 0) + 1
        
        # Select top terms
        sorted_terms = sorted(
            term_freq.items(),
            key=lambda x: x[1],
            reverse=True
        )
        
        return [term for term, _ in sorted_terms[:10]]
    
    def _extract_facts(self, text: str) -> List[str]:
        """Extract factual statements from text"""
        # Split into sentences
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        
        facts = []
        fact_indicators = [
            'is', 'are', 'was', 'were', 'has', 'have',
            'can', 'will', 'must', 'should'
        ]
        
        for sentence in sentences:
            # Split on whitespace and punctuation
            words = re.findall(r'\b\w+\b', sentence.lower())
            # Check if sentence contains fact indicators
            if any(indicator in words for indicator in fact_indicators):
                facts.append(sentence)
        
        return facts
    
    def _calculate_term_agreement(self, term_lists: List[List[str]]) -> float:
        """Calculate agreement score for key terms"""
        if not term_lists:
            return 0.0
        
        # Create term frequency map
        term_freq = {}
        total_sources = len(term_lists)
        
        for terms in term_lists:
            for term in set(terms):  # Use set to count each term once per source
                term_freq[term] = term_freq.get(term, 0) + 1
        
        # Calculate agreement scores
        agreement_scores = []
        for freq in term_freq.values():
            agreement = freq / total_sources
            if agreement >= self.similarity_threshold:
                agreement_scores.append(agreement)
        
        if not agreement_scores:
            return 0.0
        
        return sum(agreement_scores) / len(agreement_scores)
    
    def _calculate_fact_agreement(self, fact_lists: List[List[str]]) -> float:
        """Calculate agreement score for facts"""
        if not fact_lists:
            return 0.0
        
        # Compare each fact with facts from other sources
        agreement_scores = []
        total_sources = len(fact_lists)
        
        for i, facts1 in enumerate(fact_lists):
            for fact1 in facts1:
                agreements = 0
                for j, facts2 in enumerate(fact_lists):
                    if i != j:  # Don't compare with self
                        # Check if any fact in facts2 is similar
                        if any(self._calculate_fact_similarity(fact1, fact2) >= self.similarity_threshold
                              for fact2 in facts2):
                            agreements += 1
                
                if agreements > 0:  # Only count facts with some agreement
                    agreement_scores.append(agreements / (total_sources - 1))
        
        if not agreement_scores:
            return 0.0
        
        return sum(agreement_scores) / len(agreement_scores)
    
    def _calculate_fact_similarity(self, fact1: str, fact2: str) -> float:
        """Calculate similarity between two facts"""
        # Convert to sets of words (excluding stop words)
        words1 = set(re.findall(r'\b\w+\b', fact1.lower()))
        words2 = set(re.findall(r'\b\w+\b', fact2.lower()))
        words1 = words1 - self.stop_words
        words2 = words2 - self.stop_words
        
        if not words1 or not words2:
            return 0.0
        
        # Calculate Jaccard similarity
        intersection = len(words1 & words2)
        union = len(words1 | words2)
        
        return intersection / union 
    
    def _calculate_source_weight(self, source):
        """Calculate weight for a source based on type and quality."""
        base_weight = 1.0
        
        # Context posts get special weighting
        if source.get('is_context'):
            # Higher base weight for context since it's from the author
            base_weight = 0.6
            
            # Add bonuses for additional content
            if source.get('media_urls'):
                base_weight += 0.1  # Bonus for including media
            if source.get('content_text'):
                # Bonus for substantial text (max 0.2)
                text_length = len(source.get('content_text', ''))
                base_weight += min(0.2, text_length / 1000 * 0.1)
                
            return min(1.0, base_weight)
        
        # Regular content weighting
        quality_score = source.get('quality_score', 0.5)
        return base_weight * quality_score
    
    def _calculate_consensus_score(self, sources, votes):
        """Calculate consensus score based on sources and votes."""
        if not sources and not votes:
            return 0.0
        
        source_score = 0.0
        total_source_weight = 0.0
        
        # Calculate source contribution with special handling for context
        for source in sources:
            weight = self._calculate_source_weight(source)
            # Context posts contribute more to the source score
            if source.get('is_context'):
                source_score += weight * 1.5  # 50% bonus for context
            else:
                source_score += weight
            total_source_weight += 1
        
        # Normalize source score
        source_score = source_score / max(1, total_source_weight)
        
        # Calculate vote contribution
        vote_score = super()._calculate_consensus_score(votes)
        
        # Combine scores with higher weight for votes
        return 0.3 * source_score + 0.7 * vote_score 