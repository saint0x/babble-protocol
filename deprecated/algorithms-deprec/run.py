"""
FastAPI Server Runner for Babel Protocol

This script runs the FastAPI server with proper configuration and logging.
"""

import os
import uvicorn
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

if __name__ == "__main__":
    # Get configuration from environment
    host = os.getenv("API_HOST", "localhost")
    port = int(os.getenv("API_PORT", "8000"))
    debug = os.getenv("DEBUG", "false").lower() == "true"
    
    # Run server
    uvicorn.run(
        "algorithms.main:app",
        host=host,
        port=port,
        reload=debug,
        log_level="debug" if debug else "info",
        workers=1 if debug else None
    ) 