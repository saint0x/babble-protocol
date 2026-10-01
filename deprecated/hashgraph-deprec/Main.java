import model.Transaction;
import service.LedgerService;
import service.ConsensusService;

import java.io.*;
import java.security.*;
import java.util.ArrayList;
import java.util.List;
import java.util.Scanner;

public class Main {
  private static final String HASHGRAPH_FILE_PATH = "hashgraph.txt";
  private static final LedgerService ledgerService = new LedgerService();
  private static final PrivateKey mockPrivateKey = generateMockPrivateKey();

  public static void main(String[] args) {
    // Initialize services
    LedgerService ledgerService = new LedgerService();
    ConsensusService consensusService = new ConsensusService();

    // Check if the hashgraph file exists
    File hashgraphFile = new File(HASHGRAPH_FILE_PATH);
    if (hashgraphFile.exists()) {
      System.out.println("Hashgraph file already exists.");
    } else {
      System.out.println("Creating a new hashgraph file.");
      try {
        hashgraphFile.createNewFile();
      } catch (IOException e) {
        System.err.println("Error creating hashgraph file: " + e.getMessage());
        return;
      }
    }

    // Process transactions
    List<Transaction> transactions = new ArrayList<>();
    for (int i = 1; i <= 2; i++) {
      // Mock data for transactions
      Transaction transaction = new Transaction("senderUserId" + i, "recipientUserId" + i,
          "Sample transaction content " + i);
      // Submit transaction to ledger with mock private key
      ledgerService.submitTransaction(transaction, mockPrivateKey); // Pass mockPrivateKey here
      transactions.add(transaction);
    }

    // Save hashgraph to a file
    saveHashgraphToFile(transactions);

    // Ask if the user wants to query the hashgraph
    queryHashgraph();
  }

  // Method to save hashgraph to a file
  private static void saveHashgraphToFile(List<Transaction> transactions) {
    try (PrintWriter writer = new PrintWriter(new FileWriter(HASHGRAPH_FILE_PATH))) {
      // Write transactions to the file
      for (Transaction transaction : transactions) {
        writer.println(transaction.getTransactionId() + " - " + transaction.getContent());
      }
      System.out.println("Hashgraph saved to file: " + HASHGRAPH_FILE_PATH);
    } catch (IOException e) {
      System.err.println("Error occurred while saving hashgraph to file: " + e.getMessage());
    }
  }

  // Method to query the hashgraph
  private static void queryHashgraph() {
    Scanner scanner = new Scanner(System.in);
    System.out.print("Do you want to query the hashgraph? (yes/no): ");
    String input = scanner.nextLine().trim().toLowerCase();
    if (input.equals("yes")) {
      // Display hashgraph from file
      displayHashgraphFromFile();
    } else if (input.equals("no")) {
      // Ask if the user wants to add another node
      System.out.print("Do you want to add another node? (yes/no): ");
      input = scanner.nextLine().trim().toLowerCase();
      if (input.equals("yes")) {
        // Create a new transaction and add it to the ledger
        System.out.println("Creating a new node...");
        Transaction newTransaction = createNewTransaction();
        if (newTransaction != null) {
          // Submit new transaction to ledger
          ledgerService.submitTransaction(newTransaction, mockPrivateKey);
          // Append the new transaction to the hashgraph file
          saveHashgraphToFile(List.of(newTransaction));
        }
        // Query and display hashgraph
        queryHashgraph();
      } else {
        System.out.println("Exiting program.");
      }
    } else {
      System.out.println("Invalid input. Exiting program.");
    }
  }

  // Method to create a new transaction based on user input
  private static Transaction createNewTransaction() {
    Scanner scanner = new Scanner(System.in);
    System.out.println("Enter details for the new transaction:");
    System.out.print("Sender User ID: ");
    String senderUserId = scanner.nextLine().trim();
    System.out.print("Recipient User ID: ");
    String recipientUserId = scanner.nextLine().trim();
    System.out.print("Transaction Content: ");
    String content = scanner.nextLine().trim();
    if (!senderUserId.isEmpty() && !recipientUserId.isEmpty() && !content.isEmpty()) {
      // Create and return a new transaction object
      return new Transaction(senderUserId, recipientUserId, content);
    } else {
      System.out.println("Invalid input. Transaction creation failed.");
      return null;
    }
  }

  // Method to display hashgraph from file
  private static void displayHashgraphFromFile() {
    try (BufferedReader reader = new BufferedReader(new FileReader(HASHGRAPH_FILE_PATH))) {
      String line;
      System.out.println("Hashgraph:");
      while ((line = reader.readLine()) != null) {
        System.out.println(line);
      }
    } catch (IOException e) {
      System.err.println("Error occurred while reading hashgraph from file: " + e.getMessage());
    }
  }

  // Method to generate a mock private key for testing purposes
  private static PrivateKey generateMockPrivateKey() {
    try {
      KeyPairGenerator keyPairGenerator = KeyPairGenerator.getInstance("RSA");
      keyPairGenerator.initialize(2048);
      KeyPair keyPair = keyPairGenerator.generateKeyPair();
      return keyPair.getPrivate();
    } catch (NoSuchAlgorithmException e) {
      e.printStackTrace();
      return null;
    }
  }
}
