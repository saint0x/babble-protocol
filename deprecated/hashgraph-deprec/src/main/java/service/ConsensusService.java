package service;

import model.Transaction;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

public class ConsensusService {
    // Method to validate transactions and order them based on timestamps
    public List<Transaction> validateAndOrderTransactions(List<Transaction> transactions) {
        // Validate transactions and filter out invalid ones
        List<Transaction> validatedTransactions = validateTransactions(transactions);

        // Order transactions based on timestamps
        validatedTransactions.sort(Comparator.comparing(Transaction::getTimestamp));

        // Print ordered transactions for demonstration
        System.out.println("Validated and ordered transactions:");
        for (Transaction transaction : validatedTransactions) {
            System.out.println(transaction.getTransactionId() + " - " + transaction.getContent());
        }

        return validatedTransactions;
    }

    // Method to validate transactions
    private List<Transaction> validateTransactions(List<Transaction> transactions) {
        List<Transaction> validatedTransactions = new ArrayList<>();

        // Real logic for transaction validation
        for (Transaction transaction : transactions) {
            if (isValidTransaction(transaction)) {
                validatedTransactions.add(transaction);
            } else {
                System.out.println("Invalid transaction: " + transaction.getTransactionId());
            }
        }

        return validatedTransactions;
    }

    // Method to validate a single transaction
    private boolean isValidTransaction(Transaction transaction) {
        // Real logic for validating a single transaction
        // For example, validate content length, format, etc.
        return transaction.getContent() != null && !transaction.getContent().isEmpty();
    }

    // Method to simulate Byzantine fault tolerance
    private boolean simulateByzantineFaultTolerance() {
        // Real logic for simulating Byzantine fault tolerance
        // For demonstration, return a random boolean value
        return Math.random() > 0.5; // Randomly return true or false
    }

    // Method to reach consensus on a list of transactions
    public void reachConsensus(List<Transaction> transactions) {
        // Simulate consensus process until Byzantine fault tolerance is reached
        while (!simulateByzantineFaultTolerance()) {
            transactions = validateAndOrderTransactions(transactions);
        }

        // Print confirmation message when consensus is reached
        System.out.println("Consensus reached on transactions.");
    }
}
