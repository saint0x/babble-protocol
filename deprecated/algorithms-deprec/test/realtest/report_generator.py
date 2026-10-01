"""
Unified report generator for all algorithm tests
"""
import os
from datetime import datetime
from pathlib import Path
from typing import Dict, Any, List

class UnifiedReport:
    """Generate and manage a unified test report for all algorithms"""
    
    def __init__(self):
        self.report_sections = []
        self.start_time = datetime.now()
    
    def add_section(self, title: str, content: Dict[str, Any], section_type: str = 'algorithm'):
        """Add a new section to the report"""
        self.report_sections.append({
            'title': title,
            'content': content,
            'type': section_type
        })
    
    def _format_accuracy_table(self, accuracies: Dict[str, float]) -> str:
        """Format accuracy metrics as a markdown table"""
        table = [
            "| Category | Accuracy | Grade |",
            "|----------|-----------|-------|"
        ]
        
        for category, accuracy in accuracies.items():
            grade = self._get_grade(accuracy)
            table.append(f"| {category.replace('_', ' ').title()} | {accuracy:.2%} | {grade} |")
        
        return "\n".join(table)
    
    def _get_grade(self, accuracy: float) -> str:
        """Convert accuracy to letter grade"""
        if accuracy >= 0.97: return "A+"
        elif accuracy >= 0.93: return "A"
        elif accuracy >= 0.90: return "A-"
        elif accuracy >= 0.87: return "B+"
        elif accuracy >= 0.83: return "B"
        elif accuracy >= 0.80: return "B-"
        elif accuracy >= 0.77: return "C+"
        elif accuracy >= 0.73: return "C"
        elif accuracy >= 0.70: return "C-"
        elif accuracy >= 0.67: return "D+"
        elif accuracy >= 0.63: return "D"
        elif accuracy >= 0.60: return "D-"
        else: return "F"
    
    def _format_failed_scenarios(self, scenarios: List[str]) -> str:
        """Format failed scenarios as a markdown list"""
        if not scenarios:
            return "No failed scenarios! 🎉"
        return "\n".join([f"- {scenario}" for scenario in scenarios])
    
    def generate(self) -> str:
        """Generate the complete unified report"""
        execution_time = (datetime.now() - self.start_time).total_seconds()
        
        report = [
            "# 🎯 Babel Protocol Algorithm Test Results",
            f"\n## 📊 Test Overview",
            f"- **Test Date**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
            f"- **Total Test Duration**: {execution_time:.2f} seconds",
            f"- **Number of Algorithms Tested**: {len(self.report_sections)}",
            "\n## 🔍 Algorithm Performance Summary\n"
        ]
        
        # Add overall performance summary
        overall_stats = {
            'total_success': 0,
            'total_tests': 0,
            'total_time': 0.0
        }
        
        for section in self.report_sections:
            if section['type'] == 'algorithm':
                content = section['content']
                overall_stats['total_success'] += content.get('success', 0)
                overall_stats['total_tests'] += content.get('success', 0) + content.get('failed', 0)
                overall_stats['total_time'] += content.get('total_execution_time', 0)
        
        if overall_stats['total_tests'] > 0:
            overall_success_rate = overall_stats['total_success'] / overall_stats['total_tests']
            report.extend([
                f"- **Overall Success Rate**: {overall_success_rate:.2%}",
                f"- **Total Tests Executed**: {overall_stats['total_tests']}",
                f"- **Total Processing Time**: {overall_stats['total_time']:.2f} seconds\n"
            ])
        
        # Add individual algorithm sections
        for section in self.report_sections:
            content = section['content']
            report.extend([
                f"\n## {section['title']}",
                "\n### Performance Metrics",
                self._format_accuracy_table(content.get('category_accuracy', {})),
                "\n### Failed Scenarios",
                self._format_failed_scenarios(content.get('failed_scenarios', [])),
                "\n### Processing Statistics",
                f"- **Success Rate**: {content.get('success_rate', 0):.2%}",
                f"- **Average Processing Time**: {content.get('average_processing_time', 0):.3f} seconds",
                f"- **Total Execution Time**: {content.get('total_execution_time', 0):.2f} seconds\n"
            ])
        
        # Add recommendations section
        report.extend([
            "\n## 💡 Recommendations",
            "\nBased on the test results, here are the key areas for improvement:"
        ])
        
        for section in self.report_sections:
            content = section['content']
            low_performing = [
                (cat.replace('_', ' ').title(), acc)
                for cat, acc in content.get('category_accuracy', {}).items()
                if acc < 0.8
            ]
            if low_performing:
                report.append(f"\n### {section['title']}")
                for category, accuracy in low_performing:
                    report.append(f"- Improve {category} performance (current accuracy: {accuracy:.2%})")
        
        return "\n".join(report)

def save_unified_report(report: str, output_dir: Path = None):
    """Save the unified report to a file"""
    if output_dir is None:
        output_dir = Path(__file__).parent
    
    report_path = output_dir / 'ALGORITHM_TEST_RESULTS.md'
    with open(report_path, 'w') as f:
        f.write(report)
    
    return report_path 