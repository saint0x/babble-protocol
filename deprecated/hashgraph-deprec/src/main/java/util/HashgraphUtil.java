package util;

import model.Transaction;

import java.util.List;

public class HashgraphUtil {
    // Method to add a transaction to the hashgraph
    public static void addToHashgraph(List<Transaction> hashgraph, Transaction transaction) {
        if (validateTransaction(hashgraph, transaction)) {
            hashgraph.add(transaction);
            System.out.println("Transaction added to hashgraph: " + transaction.getTransactionId());
        } else {
            System.out.println("Invalid transaction: " + transaction.getTransactionId());
        }
    }

    // Method to get the last transaction added to the hashgraph
    public static Transaction getLastTransaction(List<Transaction> hashgraph) {
        if (!hashgraph.isEmpty()) {
            return hashgraph.get(hashgraph.size() - 1);
        }
        return null;
    }

    // Method to validate transaction based on the hashgraph
    private static boolean validateTransaction(List<Transaction> hashgraph, Transaction transaction) {
        // For simplicity, let's assume all transactions are valid
        // In a real-world scenario, implement actual validation logic
        return true;
    }

    // Method to order transactions in the hashgraph
    public static void orderTransactions(List<Transaction> hashgraph) {
        hashgraph.sort((t1, t2) -> Long.compare(t1.getTimestamp(), t2.getTimestamp()));
        System.out.println("Transactions ordered in the hashgraph.");
    }
}
