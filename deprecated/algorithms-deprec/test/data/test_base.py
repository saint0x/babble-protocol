"""
Test Base Algorithm Class for Babel Protocol
"""

import logging
import time
from typing import Any, Dict, Optional, Union
from pathlib import Path

from pydantic import BaseModel

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)

class AlgorithmMetrics(BaseModel):
    """Metrics tracked for each algorithm"""
    execution_time: float
    success_rate: float
    error_count: int
    last_execution: float
    cache_hits: int
    cache_misses: int

class TestBabelAlgorithm:
    """Test base class for all Babel Protocol algorithms"""
    
    def __init__(
        self,
        db_config,
        cache_ttl: int = 3600
    ):
        self.logger = logging.getLogger(self.__class__.__name__)
        self.cache_ttl = cache_ttl
        self.metrics = AlgorithmMetrics(
            execution_time=0.0,
            success_rate=1.0,
            error_count=0,
            last_execution=time.time(),
            cache_hits=0,
            cache_misses=0
        )
        
        self.db_config = db_config
        self.db = self.db_config.get_connection()
        self.cursor = self.db.cursor()

    def get_cache(self, key: str) -> Optional[Any]:
        """Get value from cache"""
        try:
            self.cursor.execute(
                "SELECT value, expiry FROM algorithm_cache WHERE key = ?",
                (f"{self.__class__.__name__}:{key}",)
            )
            result = self.cursor.fetchone()
            
            if result and result[1] > time.time():
                self.metrics.cache_hits += 1
                return result[0]
                
            self.metrics.cache_misses += 1
            return None
        except Exception as e:
            self.logger.error(f"Cache get error: {e}")
            return None

    def set_cache(self, key: str, value: Any) -> bool:
        """Set value in cache"""
        try:
            expiry = time.time() + self.cache_ttl
            self.cursor.execute(
                """
                INSERT OR REPLACE INTO algorithm_cache (key, value, expiry)
                VALUES (?, ?, ?)
                """,
                (f"{self.__class__.__name__}:{key}", value, expiry)
            )
            self.db.commit()
            return True
        except Exception as e:
            self.logger.error(f"Cache set error: {e}")
            return False

    def record_metric(self, name: str, value: Union[int, float]) -> None:
        """Record a metric for monitoring"""
        try:
            self.cursor.execute(
                """
                INSERT INTO algorithm_metrics (algorithm_name, metric_name, value, timestamp)
                VALUES (?, ?, ?, ?)
                """,
                (self.__class__.__name__, name, value, time.time())
            )
            self.db.commit()
        except Exception as e:
            self.logger.error(f"Metric recording error: {e}")

    def log_error(self, error: Exception, context: Dict[str, Any] = None) -> None:
        """Log an error with context"""
        self.metrics.error_count += 1
        self.metrics.success_rate = max(
            0.0,
            1.0 - (self.metrics.error_count / max(1, self.metrics.cache_hits + self.metrics.cache_misses))
        )
        
        error_data = {
            'error_type': type(error).__name__,
            'error_message': str(error),
            'context': context or {},
            'timestamp': time.time()
        }
        
        self.logger.error(f"Algorithm error: {error_data}")
        
        try:
            self.cursor.execute(
                """
                INSERT INTO algorithm_errors 
                (algorithm_name, error_type, error_message, context, timestamp)
                VALUES (?, ?, ?, ?, ?)
                """,
                (
                    self.__class__.__name__,
                    error_data['error_type'],
                    error_data['error_message'],
                    str(error_data['context']),
                    error_data['timestamp']
                )
            )
            self.db.commit()
        except Exception as e:
            self.logger.error(f"Error logging to database: {e}")

    def get_metrics(self) -> AlgorithmMetrics:
        """Get current algorithm metrics"""
        return self.metrics

    def __enter__(self):
        """Context manager enter"""
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Context manager exit"""
        if self.db:
            self.db.close()

    def validate_input(self, data: Any) -> bool:
        """Validate input data - to be implemented by child classes"""
        raise NotImplementedError

    def process(self, data: Any) -> Any:
        """Process data - to be implemented by child classes"""
        raise NotImplementedError

    def execute(self, data: Any) -> Any:
        """Execute algorithm with timing and error handling"""
        if not self.validate_input(data):
            raise ValueError("Invalid input data")
            
        start_time = time.time()
        try:
            result = self.process(data)
            self.metrics.execution_time = time.time() - start_time
            self.metrics.last_execution = time.time()
            return result
        except Exception as e:
            self.log_error(e, {'input_data': data})
            raise 