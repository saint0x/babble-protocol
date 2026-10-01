"""
Base Algorithm Class for Babel Protocol

This module provides the base class that all algorithm implementations will inherit from.
It handles common functionality such as:
1. Redis connection for real-time metrics
2. PostgreSQL connection for persistent storage
3. Logging and monitoring
4. Basic caching
5. Error handling
"""

import logging
import time
from typing import Any, Dict, Optional, Union, List

import psycopg2
import redis
from pydantic import BaseModel
import os
import json

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

class AlgorithmResponse(BaseModel):
    """Standard response format for all algorithms"""
    algorithm_id: str
    timestamp: float
    results: List[Any]
    metrics: Optional[Dict[str, Any]] = None
    error: Optional[str] = None

class BaseAlgorithm:
    """Base class for all Babel Protocol algorithms"""
    
    def __init__(
        self,
        redis_url: str = "redis://localhost:6379/0",
        postgres_url: str = "postgresql://localhost/babel",
        cache_ttl: int = 3600,
        db_config=None  # Test database configuration
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
        
        # Test mode takes precedence
        if db_config or os.environ.get('BABEL_TEST_MODE'):
            # Test mode - use SQLite only
            self.db_config = db_config
            self.db = self.db_config.get_connection() if db_config else None
            self.cursor = self.db.cursor() if self.db else None
            self.redis = None
            self.logger.info("Using SQLite test database")
            return  # Skip Redis and PostgreSQL setup
            
        # Production mode - use Redis and PostgreSQL
        try:
            self.redis = redis.from_url(redis_url)
            self.logger.info("Redis connection established")
        except Exception as e:
            self.logger.error(f"Redis connection failed: {e}")
            self.redis = None

        try:
            self.db = psycopg2.connect(postgres_url)
            self.logger.info("PostgreSQL connection established")
        except Exception as e:
            self.logger.error(f"PostgreSQL connection failed: {e}")
            self.db = None

    def get_cache(self, key: str) -> Optional[Any]:
        """Get value from cache"""
        if not self.redis:
            return None
            
        try:
            value = self.redis.get(f"{self.__class__.__name__}:{key}")
            if value:
                self.metrics.cache_hits += 1
                return value
            self.metrics.cache_misses += 1
            return None
        except Exception as e:
            self.logger.error(f"Cache get error: {e}")
            return None

    def set_cache(self, key: str, value: Any) -> bool:
        """Set value in cache"""
        if not self.redis:
            return False
            
        try:
            self.redis.setex(
                f"{self.__class__.__name__}:{key}",
                self.cache_ttl,
                value
            )
            return True
        except Exception as e:
            self.logger.error(f"Cache set error: {e}")
            return False

    def record_metric(self, name: str, value: Union[int, float]) -> None:
        """Record a metric for monitoring"""
        if not self.redis:
            return
            
        try:
            self.redis.hincrby(
                f"{self.__class__.__name__}:metrics",
                name,
                value
            )
        except Exception as e:
            self.logger.error(f"Metric recording error: {e}")

    def log_error(self, error: Exception, context: Dict[str, Any] = None) -> None:
        """Log an error with context"""
        if not context:
            context = {}
            
        error_data = {
            'error_type': error.__class__.__name__,
            'error_message': str(error),
            'context': json.dumps(context),
            'timestamp': time.time()
        }
        
        try:
            if self.db:
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
                        error_data['context'],
                        error_data['timestamp']
                    )
                )
                self.db.commit()
        except Exception as e:
            self.logger.error(f"Error logging to database: {e}")
        
        self.logger.error(f"Algorithm error: {error_data}")
        self.metrics.error_count += 1
        self.metrics.success_rate = max(
            0.0,
            1.0 - (self.metrics.error_count / max(1, self.metrics.cache_hits + self.metrics.cache_misses))
        )

    def log_warning(self, message: str, context: Dict[str, Any] = None) -> None:
        """Log a warning with context"""
        if not context:
            context = {}
            
        warning_data = {
            'message': message,
            'context': json.dumps(context),
            'timestamp': time.time()
        }
        
        self.logger.warning(f"Algorithm warning: {warning_data}")
        
        try:
            if self.db:
                self.cursor.execute(
                    """
                    INSERT INTO algorithm_errors 
                    (algorithm_name, error_type, error_message, context, timestamp)
                    VALUES (?, ?, ?, ?, ?)
                    """,
                    (
                        self.__class__.__name__,
                        'WARNING',
                        warning_data['message'],
                        warning_data['context'],
                        warning_data['timestamp']
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
        if self.redis:
            self.redis.close()

    def validate_input(self, data: Any) -> bool:
        """Validate input data - to be implemented by child classes"""
        raise NotImplementedError

    def process(self, data: Any) -> AlgorithmResponse:
        """Process data - to be implemented by child classes"""
        raise NotImplementedError

    def execute(self, data: Any) -> AlgorithmResponse:
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